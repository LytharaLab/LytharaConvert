//! HTTP 接口：只服务本机浏览器（127.0.0.1），无跨域头，外部站点读不到返回内容。

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Multipart, Path as RoutePath, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::catalog;
use crate::converter;
use crate::converter::Options;
use crate::settings::{Settings, THEMES};
use crate::state::{self, AppState, Job};

const MAX_FILES: usize = 3000;
const MAX_DEPTH: usize = 10;
const MAX_ENTRIES: usize = 2000;

pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn bad(message: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: message.into() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({ "error": self.message }))).into_response()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    ready: bool,
    probe_ready: bool,
    ffmpeg: String,
    ffprobe: String,
    ffmpeg_source: String,
    ffprobe_source: String,
}

pub fn engine_info(settings: &Settings) -> EngineInfo {
    let engine = state::binaries_for(settings);
    EngineInfo {
        ready: engine.ready(),
        probe_ready: engine.probe_ready(),
        ffmpeg: engine.ffmpeg.to_string_lossy().to_string(),
        ffprobe: engine.ffprobe.to_string_lossy().to_string(),
        ffmpeg_source: engine.ffmpeg_source.to_string(),
        ffprobe_source: engine.ffprobe_source.to_string(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatePayload {
    jobs: Vec<Job>,
    settings: Settings,
    formats: catalog::Formats,
    engine: EngineInfo,
    port: u16,
}

pub async fn state(State(app): State<Arc<AppState>>) -> Json<StatePayload> {
    let settings = app.settings_snapshot();
    Json(StatePayload {
        jobs: app.jobs_snapshot(),
        formats: catalog::formats(),
        engine: engine_info(&settings),
        settings,
        port: app.port.load(std::sync::atomic::Ordering::Relaxed),
    })
}

pub async fn events(
    State(app): State<Arc<AppState>>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let initial = Event::default()
        .data(serde_json::json!({ "type": "queue", "jobs": app.jobs_snapshot() }).to_string());
    let live = BroadcastStream::new(app.events.subscribe()).filter_map(|message| async move {
        match message {
            Ok(text) => Some(Ok(Event::default().data(text))),
            // 客户端跟不上就跳过快照，下一帧会补上完整状态。
            Err(_) => None,
        }
    });
    let stream = futures_util::stream::once(async move { Ok(initial) }).chain(live);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    output_dir: Option<String>,
    theme: Option<String>,
    port: Option<u16>,
    open_browser: Option<bool>,
    ffmpeg_path: Option<String>,
    ffprobe_path: Option<String>,
    concurrency: Option<u32>,
    notify_on_finish: Option<bool>,
    keep_uploads: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResponse {
    settings: Settings,
    /// 端口改动要重启服务才生效，界面据此给出提示。
    restart_required: bool,
    restarted_port: u16,
    engine: EngineInfo,
}

pub async fn update_settings(
    State(app): State<Arc<AppState>>,
    Json(patch): Json<SettingsPatch>,
) -> Result<Json<SettingsResponse>, ApiError> {
    let previous = app.settings_snapshot();
    let updated = {
        let mut guard = app.settings.lock().map_err(|_| ApiError::bad("设置暂不可用"))?;
        if let Some(value) = patch.output_dir {
            if !value.trim().is_empty() {
                guard.output_dir = value;
            }
        }
        if let Some(value) = patch.theme {
            if THEMES.contains(&value.as_str()) {
                guard.theme = value;
            }
        }
        if let Some(value) = patch.port {
            guard.port = value;
        }
        if let Some(value) = patch.open_browser {
            guard.open_browser = value;
        }
        if let Some(value) = patch.ffmpeg_path {
            guard.ffmpeg_path = value;
        }
        if let Some(value) = patch.ffprobe_path {
            guard.ffprobe_path = value;
        }
        if let Some(value) = patch.concurrency {
            guard.concurrency = value;
        }
        if let Some(value) = patch.notify_on_finish {
            guard.notify_on_finish = value;
        }
        if let Some(value) = patch.keep_uploads {
            guard.keep_uploads = value;
        }
        guard.sanitize();
        guard.clone()
    };

    // 落盘失败就把内存改回去，避免「界面上改了、重启又变回来」的错觉。
    if let Err(err) = app.save_settings(&updated) {
        if let Ok(mut guard) = app.settings.lock() {
            *guard = previous;
        }
        return Err(ApiError::bad(format!(
            "设置未能写入 {}：{err}",
            app.settings_file.display()
        )));
    }

    let running_port = app.port.load(std::sync::atomic::Ordering::Relaxed);
    let restart_required = updated.port != running_port;
    let engine = engine_info(&updated);
    // 自定义引擎或并发数变化时，立刻让工作协程用上新配置。
    app.kick_worker();
    Ok(Json(SettingsResponse {
        settings: updated,
        restart_required,
        restarted_port: running_port,
        engine,
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineTestPayload {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineTestResult {
    ok: bool,
    kind: String,
    path: String,
    version: Option<String>,
    error: Option<String>,
}

/// 检测一个 ffmpeg / ffprobe 可执行文件：跑 `-version` 看能不能用。
pub async fn engine_test(Json(payload): Json<EngineTestPayload>) -> Json<EngineTestResult> {
    let kind = if payload.kind == "ffprobe" { "ffprobe" } else { "ffmpeg" }.to_string();
    let explicit = payload
        .path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let resolved = match explicit {
        Some(path) => path,
        None => {
            let settings = Settings::default();
            let engine = state::binaries_for(&settings);
            if kind == "ffprobe" { engine.ffprobe } else { engine.ffmpeg }
        }
    };

    let args: Vec<String> = vec!["-version".to_string()];
    match converter::run(&resolved, &args, converter::RunOptions::default()).await {
        Ok(stdout) => {
            let version = stdout
                .lines()
                .next()
                .map(|line| line.trim().chars().take(200).collect::<String>());
            Json(EngineTestResult {
                ok: true,
                kind,
                path: resolved.to_string_lossy().to_string(),
                version,
                error: None,
            })
        }
        Err(err) => Json(EngineTestResult {
            ok: false,
            kind,
            path: resolved.to_string_lossy().to_string(),
            version: None,
            error: Some(err.message()),
        }),
    }
}

/// 重启服务：用同样的参数拉起新进程（沿用新端口），然后本进程退出。
pub async fn restart(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let port = app.settings_snapshot().port;
    let exe = std::env::current_exe().ok();
    let mut spawned = false;
    if let Some(exe) = exe {
        let mut args: Vec<String> = std::env::args()
            .skip(1)
            .filter(|arg| arg != "--no-open" && arg != "--wait-for-port")
            .collect();
        // 新进程比自己晚几百毫秒启动，给它一点等端口释放的余量。
        args.push("--wait-for-port".into());
        let mut command = std::process::Command::new(exe);
        command.args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            command.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }
        spawned = command.spawn().is_ok();
    }
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        std::process::exit(0);
    });
    Json(serde_json::json!({ "restarting": spawned, "port": port }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePayload {
    path: String,
    parent: Option<String>,
    roots: Vec<String>,
    entries: Vec<Entry>,
    truncated: bool,
    /// 当传入的是文件路径时，回传该文件，前端可直接勾选。
    focus: Option<String>,
}

#[derive(Deserialize)]
pub struct BrowseQuery {
    path: Option<String>,
}

/// Windows 盘符列表；其他平台返回根目录。
fn roots() -> Vec<String> {
    if cfg!(windows) {
        ('A'..='Z')
            .filter_map(|letter| {
                let candidate = format!("{letter}:\\");
                Path::new(&candidate).exists().then_some(candidate)
            })
            .collect()
    } else {
        vec!["/".to_string()]
    }
}

/// 首次打开浏览器时的起点：输出目录不存在就退到下载目录、用户目录，最后退到盘符根。
fn start_directory(preferred: &str) -> PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let candidates = [
        preferred.to_string(),
        PathBuf::from(&home).join("Downloads").to_string_lossy().to_string(),
        home,
    ];
    for candidate in candidates {
        if !candidate.trim().is_empty() && Path::new(&candidate).is_dir() {
            return PathBuf::from(candidate);
        }
    }
    roots()
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub async fn browse(
    State(app): State<Arc<AppState>>,
    Query(query): Query<BrowseQuery>,
) -> Result<Json<BrowsePayload>, ApiError> {
    let requested = query.path.unwrap_or_default();
    let requested = requested.trim().trim_matches('"').trim().to_string();
    // 允许直接粘贴文件路径：退到它所在的目录，并把该文件回传给前端勾选。
    let mut focus = None;
    let mut dir = if requested.is_empty() {
        start_directory(&app.settings_snapshot().output_dir)
    } else {
        PathBuf::from(&requested)
    };
    if dir.is_file() {
        focus = Some(dir.to_string_lossy().to_string());
        dir = dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
    }
    if !dir.is_dir() {
        return Err(ApiError::bad(format!("路径不存在：{}", dir.display())));
    }

    let mut entries = Vec::new();
    let mut truncated = false;
    let mut reader = tokio::fs::read_dir(&dir)
        .await
        .map_err(|err| ApiError::bad(format!("无法读取目录：{err}")))?;
    while let Ok(Some(item)) = reader.next_entry().await {
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        let Ok(kind) = item.file_type().await else { continue };
        let name = item.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let size = if kind.is_file() {
            item.metadata().await.map(|meta| meta.len()).unwrap_or(0)
        } else {
            0
        };
        entries.push(Entry {
            name,
            path: item.path().to_string_lossy().to_string(),
            is_dir: kind.is_dir(),
            size,
        });
    }
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    Ok(Json(BrowsePayload {
        path: dir.to_string_lossy().to_string(),
        parent: dir.parent().map(|value| value.to_string_lossy().to_string()),
        roots: roots(),
        entries,
        truncated,
        focus,
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanPayload {
    path: String,
}

async fn walk(folder: &Path, files: &mut Vec<String>, depth: usize) {
    if depth > MAX_DEPTH || files.len() >= MAX_FILES {
        return;
    }
    let Ok(mut reader) = tokio::fs::read_dir(folder).await else { return };
    while let Ok(Some(item)) = reader.next_entry().await {
        if files.len() >= MAX_FILES {
            return;
        }
        let path = item.path();
        match item.file_type().await {
            Ok(kind) if kind.is_dir() => Box::pin(walk(&path, files, depth + 1)).await,
            Ok(kind) if kind.is_file() => files.push(path.to_string_lossy().to_string()),
            _ => {}
        }
    }
}

/// 把一个文件夹展开成文件列表（最多 3000 项，深度 10 层）。
pub async fn scan(Json(payload): Json<ScanPayload>) -> Result<Json<Vec<String>>, ApiError> {
    let folder = PathBuf::from(&payload.path);
    if !folder.is_dir() {
        return Err(ApiError::bad(format!("文件夹不存在：{}", payload.path)));
    }
    let mut files = Vec::new();
    walk(&folder, &mut files, 0).await;
    Ok(Json(files))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddPayload {
    #[serde(default)]
    files: Vec<String>,
    mode: String,
    #[serde(default)]
    format: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    options: Options,
}

fn job_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}{:x}", std::process::id())
}

pub async fn add_jobs(
    State(app): State<Arc<AppState>>,
    Json(payload): Json<AddPayload>,
) -> Result<Json<Vec<Job>>, ApiError> {
    if payload.files.is_empty() {
        return Err(ApiError::bad("请先选择文件"));
    }
    if payload.files.len() > MAX_FILES {
        return Err(ApiError::bad(format!("一次最多 {MAX_FILES} 个文件")));
    }
    if !["convert", "compress", "gif"].contains(&payload.mode.as_str()) {
        return Err(ApiError::bad("任务参数无效"));
    }

    let mut inputs = Vec::new();
    for value in payload.files {
        if value.trim().is_empty() {
            continue;
        }
        if tokio::fs::metadata(&value)
            .await
            .map(|meta| meta.is_file())
            .unwrap_or(false)
        {
            inputs.push(value);
        }
    }
    if inputs.is_empty() {
        return Err(ApiError::bad("找不到输入文件"));
    }

    let entries: Vec<Vec<String>> = if payload.mode == "gif" {
        vec![inputs]
    } else {
        inputs.into_iter().map(|file| vec![file]).collect()
    };

    let output_dir = app.settings_snapshot().output_dir;
    let mut created = Vec::new();
    for sources in entries {
        let source = sources[0].clone();
        let input_bytes = tokio::fs::metadata(&source)
            .await
            .map(|meta| meta.len())
            .unwrap_or(0);
        created.push(Job {
            id: job_id(),
            mode: payload.mode.clone(),
            source,
            sources,
            format: payload.format.clone(),
            status: "pending".into(),
            progress: 0,
            bytes: None,
            output: None,
            error: None,
            input_bytes,
            frames: None,
            options: payload.options.clone(),
            name: payload.name.clone(),
            output_dir: output_dir.clone(),
        });
    }

    {
        let mut jobs = app.jobs.lock().map_err(|_| ApiError::bad("队列暂不可用"))?;
        jobs.extend(created.iter().cloned());
    }
    app.publish();
    app.kick_worker();
    Ok(Json(app.jobs_snapshot()))
}

pub async fn cancel_job(
    State(app): State<Arc<AppState>>,
    RoutePath(id): RoutePath<String>,
) -> Json<Vec<Job>> {
    if let Ok(mut jobs) = app.jobs.lock() {
        if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
            match job.status.as_str() {
                "pending" => job.status = "cancelled".into(),
                "running" => {
                    if let Ok(cancels) = app.cancels.lock() {
                        if let Some(cancel) = cancels.get(&id) {
                            cancel.cancel();
                        }
                    }
                }
                _ => {}
            }
        }
    }
    app.publish();
    Json(app.jobs_snapshot())
}

pub async fn clear_jobs(State(app): State<Arc<AppState>>) -> Json<Vec<Job>> {
    if let Ok(mut jobs) = app.jobs.lock() {
        jobs.retain(|job| job.status == "running" || job.status == "pending");
    }
    app.publish();
    Json(app.jobs_snapshot())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPayload {
    #[serde(default)]
    path: Option<String>,
    /// true：定位到文件（在资源管理器里选中）；false：打开这个目录。
    #[serde(default)]
    reveal: bool,
}

/// 在资源管理器里打开输出目录或定位某个文件。
pub async fn open_path(
    State(app): State<Arc<AppState>>,
    Json(payload): Json<OpenPayload>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let target = payload.path.unwrap_or_else(|| app.settings_snapshot().output_dir);
    if target.trim().is_empty() {
        return Err(ApiError::bad("没有可打开的路径"));
    }
    let is_file = tokio::fs::metadata(&target)
        .await
        .map(|meta| meta.is_file())
        .unwrap_or(false);

    let result = if cfg!(windows) {
        let argument = if is_file && payload.reveal {
            format!("/select,{target}")
        } else if is_file {
            format!("/select,{target}")
        } else {
            target.clone()
        };
        std::process::Command::new("explorer").arg(argument).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&target).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(&target).spawn()
    };
    result.map_err(|err| ApiError::bad(format!("无法打开：{err}")))?;
    Ok(Json(serde_json::json!({ "opened": target })))
}

fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if (ch as u32) < 0x20 => '_',
            ch => ch,
        })
        .collect();
    let trimmed: String = cleaned.chars().take(120).collect();
    if trimmed.trim().is_empty() {
        "upload.bin".into()
    } else {
        trimmed
    }
}

fn upload_dir() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0);
    crate::settings::uploads_dir().join(format!("{stamp}"))
}

/// 浏览器拖进来的文件没有本地路径，只能先落盘到临时目录再转换。
pub async fn upload(
    State(_app): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, ApiError> {
    let dir = upload_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|err| ApiError::bad(format!("无法创建临时目录：{err}")))?;

    let mut saved = Vec::new();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::bad(format!("上传数据损坏：{err}")))?
    {
        let Some(filename) = field.file_name().map(str::to_string) else { continue };
        let target = dir.join(safe_name(&filename));
        let mut file = tokio::fs::File::create(&target)
            .await
            .map_err(|err| ApiError::bad(format!("无法写入临时文件：{err}")))?;
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|err| ApiError::bad(format!("上传中断：{err}")))?
        {
            file.write_all(&chunk)
                .await
                .map_err(|err| ApiError::bad(format!("写入失败：{err}")))?;
        }
        file.flush().await.ok();
        saved.push(target.to_string_lossy().to_string());
    }

    if saved.is_empty() {
        return Err(ApiError::bad("没有收到文件"));
    }
    Ok(Json(serde_json::json!({ "files": saved, "dir": dir.to_string_lossy() })))
}
