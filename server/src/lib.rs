//! Lythara Convert 本地转换服务。
//!
//! 只监听 127.0.0.1：浏览器负责界面，这里负责队列、转换与文件系统访问。
//! 不发送任何 CORS 头，因此外部站点无法读取接口返回内容。

pub mod api;
pub mod catalog;
pub mod converter;
pub mod settings;
pub mod state;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use include_dir::{include_dir, Dir};
use tower_http::services::{ServeDir, ServeFile};

use state::AppState;

pub const DEFAULT_PORT: u16 = 8765;

/// 前端产物在编译期嵌进 exe：单独一个 exe 也能出界面，不再依赖旁边的 dist/。
/// 想临时改界面时，把 dist/ 放在 exe 旁边即可覆盖内嵌版本。
static EMBEDDED_UI: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../dist");

/// exe 旁边的 dist/（可选覆盖层）。
fn disk_dist() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join("dist");
    candidate.join("index.html").is_file().then_some(candidate)
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default().to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 内嵌前端的兜底路由：找不到具体文件就回 index.html（SPA 行为）。
async fn embedded_ui(uri: Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    let requested = if requested.is_empty() { "index.html" } else { requested };
    if let Some(file) = EMBEDDED_UI.get_file(requested) {
        return (
            [(header::CONTENT_TYPE, mime_for(requested))],
            file.contents().to_vec(),
        )
            .into_response();
    }
    match EMBEDDED_UI.get_file("index.html") {
        Some(index) => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            index.contents().to_vec(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "前端产物缺失：请先执行 npm run build").into_response(),
    }
}

fn build_router(app: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/api/state", get(api::state))
        .route("/api/events", get(api::events))
        .route("/api/settings", post(api::update_settings))
        .route("/api/browse", get(api::browse))
        .route("/api/scan", post(api::scan))
        .route("/api/jobs", post(api::add_jobs))
        .route("/api/jobs/clear", post(api::clear_jobs))
        .route("/api/jobs/{id}/cancel", post(api::cancel_job))
        .route("/api/open", post(api::open_path))
        .route("/api/upload", post(api::upload))
        .route("/api/engine/test", post(api::engine_test))
        .route("/api/restart", post(api::restart));

    // 默认用内嵌前端；exe 旁边放了 dist/ 就用磁盘上的（方便临时改界面）。
    match disk_dist() {
        Some(dir) => {
            let files = ServeDir::new(&dir).not_found_service(ServeFile::new(dir.join("index.html")));
            api.fallback_service(files)
        }
        None => api.fallback(embedded_ui),
    }
    .with_state(app)
}

fn open_browser(url: &str) {
    let result = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(err) = result {
        eprintln!("Lythara Convert: 无法自动打开浏览器（{err}），请手动访问 {url}");
    }
}

/// 启动服务。
///
/// 参数：`--no-open` 不自动打开浏览器；`--wait-for-port` 先等旧实例释放端口（重启时用）。
/// 端口来源：环境变量 `LYTHARA_PORT` > 设置里的 `port` > 默认 8765。
pub async fn serve() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let no_open = args.iter().any(|arg| arg == "--no-open");
    let wait_for_port = args.iter().any(|arg| arg == "--wait-for-port");

    let app = AppState::load();
    let saved = app.settings_snapshot();
    let port: u16 = std::env::var("LYTHARA_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(saved.port);
    let auto_open = saved.open_browser && !no_open;

    let overridden = disk_dist();
    let router = build_router(Arc::clone(&app));
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));

    let mut attempt = 0;
    let listener = loop {
        match tokio::net::TcpListener::bind(address).await {
            Ok(listener) => break listener,
            Err(err) => {
                // 重启时旧进程还没退干净：等它一会儿再试。
                if wait_for_port && attempt < 20 {
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    continue;
                }
                // 否则视为已有实例在跑，直接把浏览器指过去。
                eprintln!("Lythara Convert: {address} 已被占用（{err}），改为打开已有实例。");
                if auto_open {
                    open_browser(&format!("http://127.0.0.1:{port}/"));
                }
                return;
            }
        }
    };
    app.port.store(port, std::sync::atomic::Ordering::Relaxed);

    let url = format!("http://127.0.0.1:{port}/");
    println!("Lythara Convert 已启动：{url}");
    println!("数据目录：{}", settings::data_dir().display());
    println!("设置文件：{}", settings::settings_file().display());
    match &overridden {
        Some(dir) => println!("前端：使用 exe 旁的 {}（覆盖内嵌版本）", dir.display()),
        None => println!("前端：已内嵌在 exe 中"),
    }
    let engine = state::binaries_for(&saved);
    let source_label = |source: &str| match source {
        converter::SOURCE_CUSTOM => "设置指定",
        converter::SOURCE_ENV => "环境变量",
        converter::SOURCE_BUNDLED => "随附 binaries",
        converter::SOURCE_ALONGSIDE => "exe 同级",
        converter::SOURCE_DEV => "开发目录",
        _ => "PATH",
    };
    println!(
        "引擎：{}（{}）{}",
        engine.ffmpeg.display(),
        source_label(engine.ffmpeg_source),
        if engine.ready() { "" } else { "  ← 未找到，转换不可用" }
    );
    println!(
        "      {}（{}）{}",
        engine.ffprobe.display(),
        source_label(engine.ffprobe_source),
        if engine.probe_ready() { "" } else { "  ← 未找到，媒体探测会失败" }
    );
    println!("并发任务数：{}", saved.concurrency);
    if auto_open {
        open_browser(&url);
    }

    if let Err(err) = axum::serve(listener, router).await {
        eprintln!("Lythara Convert 服务异常退出：{err}");
    }
}
