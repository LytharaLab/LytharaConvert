//! ffmpeg / ffprobe 转换管线。对应 Electron 版本的 src/core/converter.cjs。
//!
//! 与旧实现的差异：图片编码不再经过 Sharp（libvips），全部改由随附的 FFmpeg
//! 完成；静态图片的「质量 + 目标体积」搜索逻辑与旧版保持一致。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

use crate::catalog;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ── 错误与取消 ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum Error {
    Abort,
    Message(String),
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Abort => write!(f, "任务已取消"),
            Error::Message(m) => write!(f, "{m}"),
            Error::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn is_abort(&self) -> bool {
        matches!(self, Error::Abort)
    }
    pub fn message(self) -> String {
        self.to_string()
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// 跨任务共享的取消开关。
#[derive(Clone, Default)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
}

impl Cancel {
    pub fn new() -> Self {
        Self { flag: Arc::new(AtomicBool::new(false)) }
    }
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

// ── 随附二进制 ────────────────────────────────────────────────────────────

/// 引擎来源，用于在界面上说明「这个路径是怎么来的」。
pub const SOURCE_CUSTOM: &str = "custom";
pub const SOURCE_ENV: &str = "env";
pub const SOURCE_BUNDLED: &str = "bundled";
pub const SOURCE_ALONGSIDE: &str = "alongside";
pub const SOURCE_DEV: &str = "dev";
pub const SOURCE_PATH: &str = "path";

#[derive(Clone, Debug)]
pub struct Binaries {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub ffmpeg_source: &'static str,
    pub ffprobe_source: &'static str,
}

impl Binaries {
    /// 引擎是否就绪（界面据此显示「FFmpeg 未就绪」）。
    pub fn ready(&self) -> bool {
        self.ffmpeg.is_file()
    }

    /// ffprobe 缺失不致命：无法探测时转换会明确报错，但界面仍要提示。
    pub fn probe_ready(&self) -> bool {
        self.ffprobe.is_file()
    }
}

/// 解析顺序：设置里的自定义路径 → 环境变量 → exe 旁的 binaries/ → exe 旁 → （仅开发构建）源码目录 → PATH。
fn resolve(
    env_key: &str,
    name: &str,
    exe_dir: Option<&Path>,
    explicit: Option<&Path>,
) -> (PathBuf, &'static str) {
    if let Some(path) = explicit {
        if !path.as_os_str().is_empty() {
            return (path.to_path_buf(), SOURCE_CUSTOM);
        }
    }
    if let Ok(value) = std::env::var(env_key) {
        if !value.trim().is_empty() {
            return (PathBuf::from(value), SOURCE_ENV);
        }
    }
    let file = format!("{name}.exe");
    if let Some(dir) = exe_dir {
        let bundled = dir.join("binaries").join(&file);
        if bundled.is_file() {
            return (bundled, SOURCE_BUNDLED);
        }
        let alongside = dir.join(&file);
        if alongside.is_file() {
            return (alongside, SOURCE_ALONGSIDE);
        }
    }
    // 只有开发构建才允许回退到源码目录：发布版必须把 binaries/ 放在 exe 旁边，
    // 否则宁可明确报告「引擎未就绪」，也不要偷偷用别的路径装作没事。
    #[cfg(debug_assertions)]
    {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries").join(&file);
        if dev.is_file() {
            return (dev, SOURCE_DEV);
        }
    }
    // 都找不到时退回名字，交给 PATH；调用方通过 ready() 报告引擎状态。
    (PathBuf::from(file), SOURCE_PATH)
}

/// 解析 FFmpeg / FFprobe：`explicit` 来自设置，优先于其它来源。
pub fn binaries_with(
    exe_dir: Option<&Path>,
    ffmpeg_override: Option<&Path>,
    ffprobe_override: Option<&Path>,
) -> Binaries {
    let (ffmpeg, ffmpeg_source) = resolve("LYTHARA_FFMPEG", "ffmpeg", exe_dir, ffmpeg_override);
    let (ffprobe, ffprobe_source) = resolve("LYTHARA_FFPROBE", "ffprobe", exe_dir, ffprobe_override);
    Binaries {
        ffmpeg,
        ffprobe,
        ffmpeg_source,
        ffprobe_source,
    }
}

/// 不带自定义路径的解析（测试与旧调用点使用）。
pub fn binaries(exe_dir: Option<&Path>) -> Binaries {
    binaries_with(exe_dir, None, None)
}

// ── 进程执行 ──────────────────────────────────────────────────────────────

pub type ProgressCb = Arc<dyn Fn(f64) + Send + Sync>;

#[derive(Default)]
pub struct RunOptions {
    pub cancel: Option<Cancel>,
    pub on_progress: Option<ProgressCb>,
    pub duration: f64,
}

fn tail(text: &str, chars: usize) -> String {
    let count = text.chars().count();
    if count <= chars {
        return text.to_string();
    }
    text.chars().skip(count - chars).collect()
}

/// 运行一个子进程，返回 stdout。`on_progress` 存在时解析 `-progress pipe:1` 输出。
pub async fn run(program: &Path, args: &[String], options: RunOptions) -> Result<String> {
    if let Some(cancel) = &options.cancel {
        if cancel.is_cancelled() {
            return Err(Error::Abort);
        }
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .spawn()
        .map_err(|err| Error::Io(format!("无法启动 {}：{err}", program.display())))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let progress = options.on_progress.clone();
    let duration = options.duration;

    let stdout_task = tokio::spawn(async move {
        let mut collected = String::new();
        let Some(stdout) = stdout else { return collected };
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            match &progress {
                None => {
                    collected.push_str(&line);
                    collected.push('\n');
                }
                Some(callback) => {
                    if let Some(value) = line.strip_prefix("out_time_us=") {
                        if let Ok(micros) = value.trim().parse::<f64>() {
                            if duration > 0.0 {
                                let ratio = (micros / (duration * 1e6)).clamp(0.0, 0.98);
                                callback(ratio);
                            }
                        }
                    }
                }
            }
        }
        collected
    });

    let stderr_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_end(&mut buffer).await;
        }
        tail(&String::from_utf8_lossy(&buffer), 12_000)
    });

    let status = loop {
        if let Some(cancel) = &options.cancel {
            if cancel.is_cancelled() {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(Error::Abort);
            }
        }
        match tokio::time::timeout(Duration::from_millis(60), child.wait()).await {
            Ok(Ok(status)) => break status,
            Ok(Err(err)) => return Err(Error::Io(format!("进程等待失败：{err}"))),
            Err(_) => continue,
        }
    };

    let stdout = stdout_task.await.unwrap_or_default();
    let stderr = stderr_task.await.unwrap_or_default();

    if let Some(cancel) = &options.cancel {
        if cancel.is_cancelled() {
            return Err(Error::Abort);
        }
    }
    if status.success() {
        Ok(stdout)
    } else {
        let summary: Vec<&str> = stderr
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let message = if summary.is_empty() {
            format!("FFmpeg 退出码 {}", status.code().unwrap_or(-1))
        } else {
            summary.join(" ")
        };
        Err(Error::Message(tail(&message, 650)))
    }
}

fn ffmpeg_args(args: &[String]) -> Vec<String> {
    let mut base: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-progress", "pipe:1", "-nostats"]
        .iter()
        .map(|value| value.to_string())
        .collect();
    base.extend(args.iter().cloned());
    base
}

async fn ffmpeg(binaries: &Binaries, args: &[String], options: RunOptions) -> Result<String> {
    run(&binaries.ffmpeg, &ffmpeg_args(args), options).await
}

// ── 媒体探测 ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct MediaInfo {
    pub duration: f64,
    pub video: bool,
    pub audio: bool,
    pub width: u32,
    pub height: u32,
    pub animated: bool,
    pub kind: String,
    pub frame_count: Option<u64>,
}

pub async fn probe(binaries: &Binaries, input: &Path, cancel: Option<&Cancel>) -> Result<MediaInfo> {
    let args: Vec<String> = ["-v", "error", "-show_format", "-show_streams", "-of", "json"]
        .iter()
        .map(|value| value.to_string())
        .chain([input.to_string_lossy().to_string()])
        .collect();
    let stdout = run(
        &binaries.ffprobe,
        &args,
        RunOptions { cancel: cancel.cloned(), ..Default::default() },
    )
    .await
    .map_err(|err| match err {
        Error::Abort => Error::Abort,
        other => Error::Message(format!("无法读取媒体：{}", other.message())),
    })?;

    let data: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|_| Error::Message("媒体元数据无法解析".into()))?;
    let streams = data.get("streams").and_then(|value| value.as_array()).cloned().unwrap_or_default();
    let ext = catalog::extension(&input.to_string_lossy());

    let number = |value: &serde_json::Value, key: &str| -> f64 {
        value
            .get(key)
            .and_then(|raw| match raw {
                serde_json::Value::String(text) => text.parse::<f64>().ok(),
                other => other.as_f64(),
            })
            .unwrap_or(0.0)
    };
    let codec_type = |value: &serde_json::Value| -> String {
        value.get("codec_type").and_then(|raw| raw.as_str()).unwrap_or_default().to_string()
    };

    let mut animated = catalog::is_animated(&ext)
        || (streams.iter().any(|stream| number(stream, "nb_frames") > 1.0)
            && catalog::category(&ext) == catalog::Category::Image);
    let video_stream = streams.iter().find(|stream| codec_type(stream) == "video");
    let frame_count = video_stream.map(|stream| number(stream, "nb_frames") as u64).filter(|value| *value > 0);

    // 动图容器（WebP/PNG/TIFF 多帧）通常靠帧数判断。
    if frame_count.unwrap_or(0) > 1 {
        animated = true;
    }
    let duration = {
        let format_duration = data.get("format").map(|value| number(value, "duration")).unwrap_or(0.0);
        let stream_duration = streams.iter().map(|stream| number(stream, "duration")).fold(0.0, f64::max);
        if format_duration > 0.0 { format_duration } else { stream_duration }
    };

    Ok(MediaInfo {
        duration,
        video: video_stream.is_some(),
        audio: streams.iter().any(|stream| codec_type(stream) == "audio"),
        width: video_stream.map(|stream| number(stream, "width") as u32).unwrap_or(0),
        height: video_stream.map(|stream| number(stream, "height") as u32).unwrap_or(0),
        animated,
        kind: match catalog::category(&ext) {
            catalog::Category::Image => "image",
            catalog::Category::Video => "video",
            catalog::Category::Audio => "audio",
            catalog::Category::Other => "other",
        }
        .to_string(),
        frame_count,
    })
}

// ── 路径与命名 ────────────────────────────────────────────────────────────

pub fn unique_destination(dir: &Path, stem: &str, ext: &str, as_directory: bool) -> PathBuf {
    let mut index = 0u32;
    loop {
        let suffix = if index == 0 { String::new() } else { format!(" ({})", index + 1) };
        let name = if as_directory {
            format!("{stem}{suffix}")
        } else {
            format!("{stem}{suffix}.{ext}")
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
        index += 1;
    }
}

pub fn clean_stem(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if (ch as u32) < 0x20 => '_',
            ch => ch,
        })
        .collect();
    let trimmed: String = cleaned.chars().take(120).collect();
    if trimmed.trim().is_empty() {
        "output".to_string()
    } else {
        trimmed
    }
}

fn temp_for(output: &Path) -> PathBuf {
    let dir = output.parent().unwrap_or_else(|| Path::new("."));
    let stem = output.file_stem().map(|value| value.to_string_lossy().to_string()).unwrap_or_default();
    let ext = output.extension().map(|value| value.to_string_lossy().to_string()).unwrap_or_default();
    let token = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0)
    );
    dir.join(format!(".{stem}.{token}.part.{ext}"))
}

/// 用临时文件换取「要么完整结果、要么什么都没有」。
async fn commit(file: &Path, dest: &Path) -> Result<PathBuf> {
    let ext = catalog::extension(&dest.to_string_lossy());
    let final_path = unique_destination(
        dest.parent().unwrap_or_else(|| Path::new(".")),
        &dest.file_stem().map(|value| value.to_string_lossy().to_string()).unwrap_or_default(),
        &ext,
        false,
    );
    tokio::fs::rename(file, &final_path)
        .await
        .map_err(|err| Error::Io(format!("写入结果失败：{err}")))?;
    Ok(final_path)
}

async fn remove(path: &Path) {
    let _ = tokio::fs::remove_file(path).await;
}

async fn file_size(path: &Path) -> u64 {
    tokio::fs::metadata(path).await.map(|meta| meta.len()).unwrap_or(0)
}

fn even(value: f64) -> u32 {
    let rounded = value.round().max(2.0) as u32;
    if rounded % 2 == 0 {
        rounded
    } else {
        rounded + 1
    }
}

// ── 静态图片：质量 + 目标体积搜索 ─────────────────────────────────────────

fn jpeg_quality(quality: f64) -> u32 {
    (2.0 + (100.0 - quality) * 0.29).round().clamp(2.0, 31.0) as u32
}
fn avif_crf(quality: f64) -> u32 {
    (63.0 - quality * 0.42).round().clamp(0.0, 63.0) as u32
}

/// 为一个静态图片输出格式构造编码参数（不含输入输出）。
fn image_encode_args(ext: &str, quality: f64, width: u32, orig_width: u32, orig_height: u32) -> Result<Vec<String>> {
    let mut args: Vec<String> = Vec::new();
    let scaled = width > 0 && orig_width > 0 && orig_height > 0;
    let target_width = if scaled { width } else { orig_width };
    let target_height = if scaled {
        even(orig_height as f64 * width as f64 / orig_width as f64)
    } else {
        orig_height
    };

    match ext {
        "jpg" => {
            if orig_width > 0 && orig_height > 0 {
                // JPEG 没有透明通道：先在高分辨率下合成白底，再编码。
                let scale = if scaled {
                    format!(",scale={target_width}:{target_height}:flags=lanczos")
                } else {
                    String::new()
                };
                args.push("-filter_complex".into());
                args.push(format!(
                    "color=c=white:s={target_width}x{target_height}[bg];[0:v]format=rgba{scale}[fg];[bg][fg]overlay=format=auto,format=yuvj420p"
                ));
            } else {
                args.extend(["-vf".to_string(), "format=yuvj420p".to_string()]);
            }
            args.extend(["-frames:v".into(), "1".into()]);
            args.extend(["-q:v".into(), jpeg_quality(quality).to_string()]);
        }
        "png" => {
            let mut chain: Vec<String> = Vec::new();
            if scaled {
                chain.push(format!("scale={target_width}:{target_height}:flags=lanczos"));
            }
            if quality < 90.0 {
                // 低质量档走调色板，等价于旧版 Sharp 的 palette 分支。
                let colours = quality.round().clamp(2.0, 256.0) as u32;
                chain.push(format!("split[a][b];[a]palettegen=max_colors={colours}:stats_mode=full[p];[b][p]paletteuse=dither=sierra2_4a"));
            }
            if !chain.is_empty() {
                args.extend(["-vf".to_string(), chain.join(",")]);
            }
            args.extend(["-frames:v".into(), "1".into()]);
            args.extend(["-c:v".into(), "png".into(), "-compression_level".into(), "9".into()]);
        }
        "webp" => {
            if scaled {
                args.extend(["-vf".to_string(), format!("scale={target_width}:{target_height}:flags=lanczos")]);
            }
            args.extend(["-frames:v".into(), "1".into()]);
            args.extend([
                "-c:v".into(),
                "libwebp".into(),
                "-q:v".into(),
                quality.round().clamp(1.0, 100.0).to_string(),
                "-compression_level".into(),
                "5".into(),
            ]);
        }
        "avif" => {
            if scaled {
                args.extend(["-vf".to_string(), format!("scale={target_width}:{target_height}:flags=lanczos")]);
            }
            args.extend(["-frames:v".into(), "1".into()]);
            args.extend([
                "-c:v".into(),
                "libaom-av1".into(),
                "-crf".into(),
                avif_crf(quality).to_string(),
                "-cpu-used".into(),
                "6".into(),
                "-still-picture".into(),
                "1".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-f".into(),
                "avif".into(),
            ]);
        }
        "tiff" => {
            if scaled {
                args.extend(["-vf".to_string(), format!("scale={target_width}:{target_height}:flags=lanczos")]);
            }
            args.extend(["-frames:v".into(), "1".into()]);
            args.extend(["-c:v".into(), "tiff".into(), "-compression_algo".into(), "lzw".into()]);
        }
        other => return Err(Error::Message(format!("该图片格式不能由内置图片管线输出：{other}"))),
    }
    Ok(args)
}

async fn convert_image(
    binaries: &Binaries,
    input: &Path,
    tmp: &Path,
    ext: &str,
    quality: f64,
    target_bytes: u64,
    orig_width: u32,
    orig_height: u32,
    cancel: Option<&Cancel>,
    on_progress: Option<ProgressCb>,
) -> Result<()> {
    let base = quality.clamp(10.0, 100.0);
    let steps = if target_bytes > 0 { 4u32 } else { 1u32 };
    let mut best: Option<(PathBuf, u64)> = None;
    let total_attempts = steps as f64 * 4.0;

    for step in 0..steps {
        let width = if step > 0 && orig_width > 0 {
            ((orig_width as f64 * 0.82_f64.powi(step as i32)).round() as u32).max(64)
        } else {
            0
        };
        let mut attempts: Vec<f64> = vec![base];
        if target_bytes > 0 {
            for factor in [0.78, 0.60] {
                let value = (base * factor).round();
                if !attempts.contains(&value) {
                    attempts.push(value);
                }
            }
            if !attempts.contains(&30.0) {
                attempts.push(30.0);
            }
        }

        for (index, attempt) in attempts.iter().enumerate() {
            if let Some(cancel) = cancel {
                if cancel.is_cancelled() {
                    if let Some((file, _)) = &best {
                        remove(file).await;
                    }
                    return Err(Error::Abort);
                }
            }
            // 候选文件必须保留真实扩展名，FFmpeg 靠它推断封装格式。
            let base = tmp.to_string_lossy().to_string();
            let suffix = format!(".{ext}");
            let prefix = base.strip_suffix(&suffix).unwrap_or(&base).to_string();
            let candidate = PathBuf::from(format!("{prefix}.{step}.{attempt}{suffix}"));
            let args: Vec<String> = ["-i".to_string(), input.to_string_lossy().to_string()]
                .into_iter()
                .chain(image_encode_args(ext, *attempt, width, orig_width, orig_height)?)
                .chain([candidate.to_string_lossy().to_string()])
                .collect();
            if let Err(err) = ffmpeg(binaries, &args, RunOptions { cancel: cancel.cloned(), ..Default::default() }).await {
                remove(&candidate).await;
                if let Some((file, _)) = &best {
                    remove(file).await;
                }
                return Err(err);
            }
            let bytes = file_size(&candidate).await;
            let keep = match &best {
                None => true,
                Some((_, best_bytes)) => {
                    (*best_bytes > target_bytes && bytes < *best_bytes)
                        || (bytes <= target_bytes && (*best_bytes > target_bytes || bytes > *best_bytes))
                }
            };
            if keep {
                if let Some((file, _)) = &best {
                    remove(file).await;
                }
                best = Some((candidate, bytes));
            } else {
                remove(&candidate).await;
            }
            if let Some(callback) = &on_progress {
                let done = step as f64 * 4.0 + index as f64 + 1.0;
                callback((done / total_attempts * 0.92).min(0.92));
            }
            if target_bytes == 0 || bytes <= target_bytes {
                if let Some((file, _)) = &best {
                    tokio::fs::rename(file, tmp)
                        .await
                        .map_err(|err| Error::Io(format!("整理输出失败：{err}")))?;
                }
                return Ok(());
            }
        }
    }

    match best {
        Some((file, _)) => tokio::fs::rename(&file, tmp)
            .await
            .map_err(|err| Error::Io(format!("整理输出失败：{err}"))),
        None => Err(Error::Message("图片编码没有产生输出".into())),
    }
}

// ── GIF ───────────────────────────────────────────────────────────────────

fn gif_filter(width: u32, fps: f64, colours: u32) -> String {
    let scale = if width > 0 {
        format!(",scale='min({width},iw)':-2:flags=lanczos")
    } else {
        String::new()
    };
    format!("[0:v]fps={fps}{scale},split[g0][g1];[g0]palettegen=max_colors={colours}:stats_mode=diff[p];[g1][p]paletteuse=dither=sierra2_4a[v]")
}

async fn encode_gif(
    binaries: &Binaries,
    input_args: &[String],
    tmp: &Path,
    width: u32,
    duration: f64,
    options: &Options,
    cancel: Option<&Cancel>,
    on_progress: Option<ProgressCb>,
) -> Result<()> {
    let target = options.target_bytes();
    let mut current_width = (options.max_width.unwrap_or(960.0).max(1.0) as u32).min(if width > 0 { width } else { 960 });
    let mut fps = options.gif_fps.unwrap_or(12.0).clamp(1.0, 30.0);
    let mut colours = (options.quality.unwrap_or(78.0) * 2.56).round().clamp(32.0, 256.0);
    let attempts = if target > 0 { 5 } else { 1 };

    for attempt in 0..attempts {
        if let Some(cancel) = cancel {
            if cancel.is_cancelled() {
                return Err(Error::Abort);
            }
        }
        let filter = gif_filter(current_width, fps, colours as u32);
        let args: Vec<String> = input_args
            .iter()
            .cloned()
            .chain([
                "-filter_complex".to_string(),
                filter,
                "-map".to_string(),
                "[v]".to_string(),
                "-an".to_string(),
                "-loop".to_string(),
                "0".to_string(),
                tmp.to_string_lossy().to_string(),
            ])
            .collect();
        let progress_cb = on_progress.clone().map(|callback| {
            let callback = callback.clone();
            let attempt = attempt as f64;
            let total = attempts as f64;
            Arc::new(move |value: f64| callback((attempt + value) / total)) as ProgressCb
        });
        ffmpeg(
            binaries,
            &args,
            RunOptions { cancel: cancel.cloned(), on_progress: progress_cb, duration },
        )
        .await?;
        if target == 0 || file_size(tmp).await <= target {
            break;
        }
        current_width = ((current_width as f64 * 0.80).round() as u32).max(16);
        fps = (fps * 0.82).max(4.0);
        colours = (colours * 0.78).max(32.0);
    }
    Ok(())
}

async fn make_gif(
    binaries: &Binaries,
    sources: &[PathBuf],
    tmp: &Path,
    options: &Options,
    cancel: Option<&Cancel>,
    on_progress: Option<ProgressCb>,
) -> Result<()> {
    let fps = options.gif_fps.unwrap_or(12.0).clamp(1.0, 30.0);
    let seconds = options
        .frame_seconds
        .unwrap_or(1.0 / fps)
        .clamp(0.05, 10.0);

    if sources.len() == 1 {
        let info = probe(binaries, &sources[0], cancel).await?;
        let single = options.single_duration.unwrap_or(3.0).max(1.0);
        let input_args: Vec<String> = [
            "-loop".to_string(),
            "1".to_string(),
            "-framerate".to_string(),
            fps.to_string(),
            "-t".to_string(),
            single.to_string(),
            "-i".to_string(),
            sources[0].to_string_lossy().to_string(),
        ]
        .to_vec();
        return encode_gif(binaries, &input_args, tmp, info.width, single, options, cancel, on_progress).await;
    }

    // 帧归一化需要工作目录：优先 exe 同级的 appdata/tmp，其次系统临时目录，最后输出目录旁。
    let folder_name = format!(
        "lythara-gif-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0)
    );
    let mut temp_dir = crate::settings::temp_dir().join(&folder_name);
    if tokio::fs::create_dir_all(&temp_dir).await.is_err() {
        temp_dir = std::env::temp_dir().join(&folder_name);
        if tokio::fs::create_dir_all(&temp_dir).await.is_err() {
            temp_dir = tmp
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&folder_name);
        }
    }
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|err| Error::Io(format!("无法创建临时目录：{err}")))?;

    let result = async {
        // 合成 GIF 需要统一画布：逐帧等比缩放后补透明边。
        let mut widths = Vec::new();
        let mut heights = Vec::new();
        for source in sources {
            let info = probe(binaries, source, cancel).await?;
            widths.push(info.width.max(1));
            heights.push(info.height.max(1));
        }
        let longest = widths.iter().copied().max().unwrap_or(1) as f64;
        let tallest = heights.iter().copied().max().unwrap_or(1) as f64;
        let canvas_width = (longest.min(options.max_width.unwrap_or(960.0).max(1.0)) as u32).max(16);
        let canvas_height = ((tallest * canvas_width as f64 / longest).round() as u32).max(16);

        let mut manifest = String::new();
        let mut last_frame = PathBuf::new();
        for (index, source) in sources.iter().enumerate() {
            if let Some(cancel) = cancel {
                if cancel.is_cancelled() {
                    return Err(Error::Abort);
                }
            }
            let local = temp_dir.join(format!("{:05}.png", index));
            let args: Vec<String> = [
                "-i".to_string(),
                source.to_string_lossy().to_string(),
                "-vf".to_string(),
                format!(
                    "scale={canvas_width}:{canvas_height}:force_original_aspect_ratio=decrease,pad={canvas_width}:{canvas_height}:(ow-iw)/2:(oh-ih)/2:color=#00000000,format=rgba"
                ),
                "-frames:v".to_string(),
                "1".to_string(),
                "-update".to_string(),
                "1".to_string(),
                local.to_string_lossy().to_string(),
            ]
            .to_vec();
            ffmpeg(binaries, &args, RunOptions { cancel: cancel.cloned(), ..Default::default() }).await?;
            manifest.push_str(&format!(
                "file '{}'\nduration {seconds}\n",
                local.to_string_lossy().replace('\\', "/").replace('\'', "'\\''")
            ));
            last_frame = local;
        }
        manifest.push_str(&format!(
            "file '{}'\n",
            last_frame.to_string_lossy().replace('\\', "/").replace('\'', "'\\''")
        ));
        let manifest_path = temp_dir.join("frames.txt");
        tokio::fs::write(&manifest_path, manifest)
            .await
            .map_err(|err| Error::Io(format!("无法写入帧清单：{err}")))?;

        let input_args: Vec<String> = [
            "-f".to_string(),
            "concat".to_string(),
            "-safe".to_string(),
            "0".to_string(),
            "-i".to_string(),
            manifest_path.to_string_lossy().to_string(),
        ]
        .to_vec();
        encode_gif(
            binaries,
            &input_args,
            tmp,
            canvas_width,
            sources.len() as f64 * seconds,
            options,
            cancel,
            on_progress,
        )
        .await
    }
    .await;

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    result
}

// ── 任务 ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub quality: Option<f64>,
    pub target_mb: Option<f64>,
    pub frames: Option<String>,
    pub gif_fps: Option<f64>,
    pub frame_seconds: Option<f64>,
    pub max_width: Option<f64>,
    pub single_duration: Option<f64>,
}

impl Options {
    fn target_bytes(&self) -> u64 {
        match self.target_mb {
            Some(value) if value > 0.0 => (value * 1024.0 * 1024.0).round() as u64,
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRequest {
    pub source: String,
    #[serde(default)]
    pub sources: Vec<String>,
    pub mode: String,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub name: String,
    pub output_dir: String,
    #[serde(default)]
    pub options: Options,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobOutput {
    pub output: String,
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frames: Option<u64>,
}

fn video_encode_args(ext: &str, options: &Options, duration: f64, has_audio: bool) -> Vec<String> {
    let Some((video_codec, audio_codec)) = catalog::video_codecs(ext) else {
        return Vec::new();
    };
    let quality = options.quality.unwrap_or(78.0);
    let mut args = vec!["-c:v".to_string(), video_codec.to_string()];
    if video_codec == "libx264" {
        args.extend([
            "-preset".into(),
            "medium".into(),
            "-crf".into(),
            (37.0 - quality * 0.24).round().to_string(),
            "-pix_fmt".into(),
            "yuv420p".into(),
        ]);
    }
    if video_codec == "libvpx-vp9" {
        args.extend([
            "-b:v".into(),
            "0".into(),
            "-crf".into(),
            (52.0 - quality * 0.38).round().to_string(),
            "-pix_fmt".into(),
            "yuv420p".into(),
        ]);
    }
    let target = options.target_bytes();
    if target > 0 && duration > 0.0 {
        // 目标体积模式使用码率控制；容器开销与编码器差异仍然存在。
        let total = target as f64 * 8.0 / duration;
        let bitrate = (total * 0.88 - if has_audio { 128_000.0 } else { 0.0 }).max(120_000.0).round() as u64;
        if let Some(index) = args.iter().position(|value| value == "-crf") {
            args.drain(index..index + 2);
        }
        if let Some(index) = args.iter().position(|value| value == "-b:v") {
            args.drain(index..index + 2);
        }
        args.extend([
            "-b:v".into(),
            bitrate.to_string(),
            "-maxrate".into(),
            ((bitrate as f64 * 1.25).round() as u64).to_string(),
            "-bufsize".into(),
            (bitrate * 2).to_string(),
        ]);
    }
    if has_audio {
        args.extend(["-c:a".into(), audio_codec.into(), "-b:a".into(), "128k".into()]);
    }
    if ["mp4", "m4v", "mov"].contains(&ext) {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args
}

async fn collect_frames(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(folder).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_type().await.map(|kind| kind.is_file()).unwrap_or(false) {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    files
}

/// 执行一个转换任务：入口与 Electron 版本一致。
pub async fn execute(
    binaries: &Binaries,
    job: &JobRequest,
    cancel: Option<&Cancel>,
    on_progress: Option<ProgressCb>,
) -> Result<JobOutput> {
    let options = job.options.clone();
    let sources: Vec<PathBuf> = if job.sources.is_empty() {
        vec![PathBuf::from(&job.source)]
    } else {
        job.sources.iter().map(PathBuf::from).collect()
    };
    let input = sources
        .first()
        .cloned()
        .ok_or_else(|| Error::Message("输入文件不存在".into()))?;
    if !tokio::fs::metadata(&input).await.map(|meta| meta.is_file()).unwrap_or(false) {
        return Err(Error::Message("输入文件不存在".into()));
    }
    if let Some(cancel) = cancel {
        if cancel.is_cancelled() {
            return Err(Error::Abort);
        }
    }

    let input_ext = catalog::extension(&input.to_string_lossy());
    let input_kind = catalog::category(&input_ext);
    let output_ext = catalog::safe_extension(if job.mode == "compress" {
        &input_ext
    } else if job.mode == "gif" {
        "gif"
    } else {
        &job.format
    })
    .map_err(Error::Message)?;
    let output_kind = catalog::category(&output_ext);
    let dir = std::path::absolute(Path::new(&job.output_dir))
        .map_err(|err| Error::Io(format!("输出目录无效：{err}")))?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|err| Error::Io(format!("无法创建输出目录：{err}")))?;

    let stem_source: String = if job.mode == "gif" {
        if job.name.is_empty() {
            "animation".to_string()
        } else {
            job.name.clone()
        }
    } else {
        format!(
            "{}{}",
            input.file_stem().map(|value| value.to_string_lossy().to_string()).unwrap_or_default(),
            if job.mode == "compress" { "_compressed" } else { "" }
        )
    };
    let stem = clean_stem(&stem_source);

    let info = probe(binaries, &input, cancel).await?;
    if !info.video && !info.audio {
        return Err(Error::Message("未识别到可以转换的媒体流".into()));
    }
    if job.mode == "gif"
        && sources
            .iter()
            .any(|file| catalog::category(&catalog::extension(&file.to_string_lossy())) != catalog::Category::Image)
    {
        return Err(Error::Message("GIF 制作器只接受图片".into()));
    }
    if output_kind == catalog::Category::Video && !info.video {
        return Err(Error::Message("该输入没有视频画面".into()));
    }
    if output_kind == catalog::Category::Audio && !info.audio {
        return Err(Error::Message("该输入没有音频轨道".into()));
    }
    if output_kind == catalog::Category::Image && !info.video {
        return Err(Error::Message("该输入没有图像画面".into()));
    }

    let first_frame_only = options.frames.as_deref() == Some("first");
    let moving_input = info.animated || input_kind == catalog::Category::Video
        || (info.kind == "other" && info.video && info.duration > 0.0);
    let extract = job.mode != "gif"
        && output_kind == catalog::Category::Image
        && output_ext != "gif"
        && moving_input
        && !first_frame_only;
    let dest = unique_destination(
        &dir,
        &format!("{stem}{}", if extract { format!("_{output_ext}_frames") } else { String::new() }),
        &output_ext,
        extract,
    );
    let tmp = if extract {
        PathBuf::from(format!("{}.part-{}", dest.to_string_lossy(), std::process::id()))
    } else {
        temp_for(&dest)
    };

    let outcome = async {
        if extract {
            tokio::fs::create_dir_all(&tmp)
                .await
                .map_err(|err| Error::Io(format!("无法创建帧目录：{err}")))?;
            let target = tmp.join(format!("frame_%06d.{output_ext}"));
            let args: Vec<String> = [
                "-i".to_string(),
                input.to_string_lossy().to_string(),
                "-map".to_string(),
                "0:v:0".to_string(),
                "-fps_mode".to_string(),
                "passthrough".to_string(),
                "-start_number".to_string(),
                "1".to_string(),
                target.to_string_lossy().to_string(),
            ]
            .to_vec();
            ffmpeg(
                binaries,
                &args,
                RunOptions { cancel: cancel.cloned(), on_progress: on_progress.clone(), duration: info.duration },
            )
            .await?;
            let files = collect_frames(&tmp).await;
            if files.is_empty() {
                return Err(Error::Message("没有提取到任何画面".into()));
            }
            tokio::fs::rename(&tmp, &dest)
                .await
                .map_err(|err| Error::Io(format!("整理帧目录失败：{err}")))?;
            let mut bytes = 0u64;
            for file in collect_frames(&dest).await {
                bytes += file_size(&file).await;
            }
            if let Some(callback) = &on_progress {
                callback(1.0);
            }
            return Ok(JobOutput {
                output: dest.to_string_lossy().to_string(),
                bytes,
                frames: Some(files.len() as u64),
            });
        }

        if job.mode == "gif" {
            make_gif(binaries, &sources, &tmp, &options, cancel, on_progress.clone()).await?;
        } else if output_ext == "gif" {
            let input_args: Vec<String> = ["-i".to_string(), input.to_string_lossy().to_string()].to_vec();
            encode_gif(
                binaries,
                &input_args,
                &tmp,
                info.width,
                info.duration,
                &options,
                cancel,
                on_progress.clone(),
            )
            .await?;
        } else if output_kind == catalog::Category::Image
            && info.kind == "image"
            && !info.animated
            && catalog::TUNED_IMAGE_OUTPUT.contains(&output_ext.as_str())
        {
            convert_image(
                binaries,
                &input,
                &tmp,
                &output_ext,
                options.quality.unwrap_or(78.0),
                options.target_bytes(),
                info.width,
                info.height,
                cancel,
                on_progress.clone(),
            )
            .await?;
        } else {
            let mut args: Vec<String> = vec!["-i".to_string(), input.to_string_lossy().to_string()];
            if output_kind == catalog::Category::Audio {
                args.extend(["-map".into(), "0:a:0".into(), "-vn".into()]);
                if let Some(codec) = catalog::audio_codec(&output_ext) {
                    args.extend(["-c:a".into(), codec.into()]);
                }
                let lossless = ["wav", "aiff", "flac", "alac"].contains(&output_ext.as_str());
                let target = options.target_bytes();
                if target > 0 && info.duration > 0.0 && !lossless {
                    let rate = ((target as f64 * 8192.0 / info.duration).round() as i64).clamp(32, 320);
                    args.extend(["-b:a".into(), format!("{rate}k")]);
                } else if job.mode == "compress" && !lossless {
                    let rate = (64.0 + options.quality.unwrap_or(78.0) * 1.6).round();
                    args.extend(["-b:a".into(), format!("{rate}k")]);
                }
            } else if output_kind == catalog::Category::Image {
                args.extend([
                    "-map".into(),
                    "0:v:0".into(),
                    "-frames:v".into(),
                    "1".into(),
                    "-an".into(),
                ]);
            } else if output_kind == catalog::Category::Video {
                args.extend(["-map".into(), "0:v:0".into(), "-map".into(), "0:a:0?".into(), "-sn".into(), "-dn".into()]);
                args.extend(video_encode_args(&output_ext, &options, info.duration, info.audio));
            }
            args.push(tmp.to_string_lossy().to_string());
            ffmpeg(
                binaries,
                &args,
                RunOptions { cancel: cancel.cloned(), on_progress: on_progress.clone(), duration: info.duration },
            )
            .await?;
        }

        let final_path = commit(&tmp, &dest).await?;
        if let Some(callback) = &on_progress {
            callback(1.0);
        }
        Ok(JobOutput {
            bytes: file_size(&final_path).await,
            output: final_path.to_string_lossy().to_string(),
            frames: None,
        })
    }
    .await;

    if extract {
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    } else {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_stem_strips_windows_reserved_characters() {
        assert_eq!(clean_stem("a<b>c:d\"e/f\\g|h?i*j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(clean_stem("   "), "output");
        assert_eq!(clean_stem("ok.mp4"), "ok.mp4");
        assert_eq!(clean_stem(&"x".repeat(200)).chars().count(), 120);
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-scratch")
            .join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unique_destination_numbers_from_two() {
        let dir = scratch_dir("unique");
        let first = unique_destination(&dir, "clip", "mp4", false);
        assert_eq!(first.file_name().unwrap().to_string_lossy(), "clip.mp4");
        std::fs::write(&first, b"x").unwrap();
        let second = unique_destination(&dir, "clip", "mp4", false);
        assert_eq!(second.file_name().unwrap().to_string_lossy(), "clip (2).mp4");
        let folder = unique_destination(&dir, "frames", "png", true);
        assert_eq!(folder.file_name().unwrap().to_string_lossy(), "frames");
        std::fs::create_dir_all(&folder).unwrap();
        let folder_again = unique_destination(&dir, "frames", "png", true);
        assert_eq!(folder_again.file_name().unwrap().to_string_lossy(), "frames (2)");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn quality_mappings_stay_inside_encoder_ranges() {
        assert_eq!(jpeg_quality(100.0), 2);
        assert!((2..=31).contains(&jpeg_quality(10.0)));
        assert_eq!(avif_crf(100.0), 21);
        assert!((0..=63).contains(&avif_crf(1.0)));
    }

    #[test]
    fn image_encode_args_use_the_expected_encoders() {
        let jpg = image_encode_args("jpg", 78.0, 0, 64, 48).unwrap();
        assert!(jpg.iter().any(|value| value.contains("color=c=white:s=64x48[bg]")));
        assert!(jpg.iter().any(|value| value == "-q:v"));
        let png = image_encode_args("png", 60.0, 0, 64, 48).unwrap();
        assert!(png.iter().any(|value| value.contains("palettegen=max_colors=60")));
        let avif = image_encode_args("avif", 78.0, 32, 64, 48).unwrap();
        assert!(avif.iter().any(|value| value == "libaom-av1"));
        assert!(avif.iter().any(|value| value == "scale=32:24:flags=lanczos"));
        assert!(image_encode_args("heic", 78.0, 0, 64, 48).is_err());
    }
}
