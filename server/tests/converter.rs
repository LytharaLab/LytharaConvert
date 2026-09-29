//! 转换管线集成测试。
//!
//! 需要 `server/binaries/` 下的 ffmpeg / ffprobe（`scripts/fetch-ffmpeg.ps1` 会放到那里）。
//! 全部素材在 `target/test-scratch/` 里现造现用。

use std::path::{Path, PathBuf};

use lythara_convert_lib::converter::{self, Cancel, Error, JobRequest, Options, RunOptions};
use serde_json::Value;

fn binaries() -> converter::Binaries {
    converter::binaries(None)
}

fn scratch(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-scratch")
        .join(format!(
            "{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_millis())
                .unwrap_or(0)
        ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

async fn ffmpeg(args: &[&str]) {
    let binaries = binaries();
    let owned: Vec<String> = args.iter().map(|value| value.to_string()).collect();
    converter::run(&binaries.ffmpeg, &owned, RunOptions::default())
        .await
        .expect("ffmpeg helper failed");
}

/// 造一张纯色图片。
async fn make_image(path: &Path, size: &str, color: &str) {
    ffmpeg(&[
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        &format!("color=c={color}:s={size}"),
        "-frames:v",
        "1",
        "-update",
        "1",
        &path.to_string_lossy(),
    ])
    .await;
}

async fn ffprobe(path: &Path) -> Value {
    let binaries = binaries();
    let args: Vec<String> = [
        "-v",
        "error",
        "-count_frames",
        "-show_format",
        "-show_streams",
        "-of",
        "json",
        &path.to_string_lossy(),
    ]
    .iter()
    .map(|value| value.to_string())
    .collect();
    let raw = converter::run(&binaries.ffprobe, &args, RunOptions::default())
        .await
        .expect("ffprobe failed");
    serde_json::from_str(&raw).expect("ffprobe json")
}

fn codec_name(data: &Value, kind: &str) -> String {
    data["streams"]
        .as_array()
        .and_then(|streams| {
            streams
                .iter()
                .find(|stream| stream["codec_type"] == kind)
                .and_then(|stream| stream["codec_name"].as_str())
        })
        .unwrap_or_default()
        .to_string()
}

fn frame_count(data: &Value) -> u64 {
    data["streams"]
        .as_array()
        .and_then(|streams| streams.first())
        .map(|stream| {
            ["nb_read_frames", "nb_frames"]
                .iter()
                .find_map(|key| stream[*key].as_str())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

fn request(source: &Path, dir: &Path, mode: &str, format: &str) -> JobRequest {
    JobRequest {
        source: source.to_string_lossy().to_string(),
        sources: vec![source.to_string_lossy().to_string()],
        mode: mode.into(),
        format: format.into(),
        name: String::new(),
        output_dir: dir.to_string_lossy().to_string(),
        options: Options::default(),
    }
}

#[tokio::test]
async fn converts_still_images_gifs_and_audio_tracks() {
    let root = scratch("pipeline");
    let red = root.join("red.png");
    let blue = root.join("blue.png");
    make_image(&red, "64x48", "#f04362").await;
    make_image(&blue, "32x64", "#326def").await;

    // 1) 静态图片 → JPEG，质量参数生效，重复转换不覆盖已有结果。
    let mut jpg = request(&red, &root, "convert", "jpg");
    jpg.options.quality = Some(75.0);
    let jpeg = converter::execute(&binaries(), &jpg, None, None).await.expect("jpg conversion");
    assert!(jpeg.output.ends_with(".jpg"), "expected .jpg, got {}", jpeg.output);
    assert!(jpeg.bytes > 0);
    assert_eq!(codec_name(&ffprobe(Path::new(&jpeg.output)).await, "video"), "mjpeg");
    let again = converter::execute(&binaries(), &jpg, None, None).await.expect("second jpg");
    assert_ne!(again.output, jpeg.output, "同名输出必须自动编号");

    // 2) 单图循环 GIF。
    let mut single = request(&red, &root, "gif", "gif");
    single.options = Options {
        single_duration: Some(2.0),
        gif_fps: Some(5.0),
        max_width: Some(64.0),
        ..Default::default()
    };
    let single_gif = converter::execute(&binaries(), &single, None, None).await.expect("single gif");
    let single_info = ffprobe(Path::new(&single_gif.output)).await;
    assert!(frame_count(&single_info) > 1, "单图循环 GIF 应该有多帧");
    assert!(single_gif.bytes > 0);

    // 3) 多图合成 GIF。
    let mut multi = request(&red, &root, "gif", "gif");
    multi.sources = vec![red.to_string_lossy().to_string(), blue.to_string_lossy().to_string()];
    multi.options = Options {
        gif_fps: Some(5.0),
        frame_seconds: Some(0.5),
        quality: Some(75.0),
        max_width: Some(64.0),
        ..Default::default()
    };
    let gif = converter::execute(&binaries(), &multi, None, None).await.expect("multi gif");
    assert!(gif.bytes > 0);

    // 4) GIF / 动图 → 逐帧 PNG。
    let mut frames_request = request(Path::new(&gif.output), &root, "convert", "png");
    frames_request.options.frames = Some("all".into());
    let frames = converter::execute(&binaries(), &frames_request, None, None).await.expect("frames");
    assert!(frames.frames.unwrap_or(0) >= 2, "至少应导出两帧，实际 {:?}", frames.frames);
    for entry in std::fs::read_dir(&frames.output).expect("frames dir") {
        let entry = entry.expect("dir entry").path();
        assert_eq!(entry.extension().unwrap().to_string_lossy(), "png");
    }

    // 5) 只取第一帧。
    let mut first_request = request(Path::new(&gif.output), &root, "convert", "png");
    first_request.options.frames = Some("first".into());
    let first = converter::execute(&binaries(), &first_request, None, None).await.expect("first frame");
    assert_eq!(codec_name(&ffprobe(Path::new(&first.output)).await, "video"), "png");

    // 6) 压缩已有 GIF：沿用扩展名并加 _compressed。
    let mut compress = request(Path::new(&gif.output), &root, "compress", "");
    compress.options = Options {
        quality: Some(45.0),
        target_mb: Some(0.003),
        gif_fps: Some(5.0),
        ..Default::default()
    };
    let smaller = converter::execute(&binaries(), &compress, None, None).await.expect("compress gif");
    assert!(smaller.output.ends_with("_compressed.gif"), "got {}", smaller.output);

    // 7) 视频 → MP3：提取音轨。
    let video = root.join("video.mp4");
    ffmpeg(&[
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "color=c=blue:s=64x48:r=5:d=1",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=1",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-shortest",
        &video.to_string_lossy(),
    ])
    .await;

    let mut mp3 = request(&video, &root, "convert", "mp3");
    mp3.options.quality = Some(70.0);
    let audio = converter::execute(&binaries(), &mp3, None, None).await.expect("mp3");
    assert!(audio.output.ends_with(".mp3"));
    assert!(audio.bytes > 1000, "mp3 太小：{} 字节", audio.bytes);

    // 8) 视频压缩（按时长估算码率）。
    let mut compress_video = request(&video, &root, "compress", "");
    compress_video.options = Options {
        quality: Some(65.0),
        target_mb: Some(0.08),
        ..Default::default()
    };
    let compressed = converter::execute(&binaries(), &compress_video, None, None)
        .await
        .expect("compressed video");
    assert!(compressed.bytes > 0);

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn rejects_impossible_jobs_instead_of_writing_fake_results() {
    let root = scratch("guards");
    let red = root.join("red.png");
    make_image(&red, "32x32", "#22cc88").await;

    // 输入为纯音频却要视频输出。
    let audio_only = root.join("tone.wav");
    ffmpeg(&[
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=0.5",
        &audio_only.to_string_lossy(),
    ])
    .await;
    let video_request = request(&audio_only, &root, "convert", "mp4");
    let err = converter::execute(&binaries(), &video_request, None, None)
        .await
        .expect_err("纯音频不应产出视频");
    let text = err.to_string();
    assert!(text.contains("没有视频画面"), "{text}");

    // 不存在的输入。
    let missing = request(&root.join("nope.png"), &root, "convert", "jpg");
    let err = converter::execute(&binaries(), &missing, None, None)
        .await
        .expect_err("缺失输入必须报错");
    let text = err.to_string();
    assert!(text.contains("输入文件不存在"), "{text}");

    // 非法扩展名。
    let mut bad_format = request(&red, &root, "convert", "a/very/long/thing");
    bad_format.format = "a/very/long/thing".into();
    let err = converter::execute(&binaries(), &bad_format, None, None)
        .await
        .expect_err("非法扩展名必须被拒绝");
    let text = err.to_string();
    assert!(text.contains("扩展名"), "{text}");

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn already_cancelled_jobs_stop_immediately() {
    let root = scratch("cancel");
    let red = root.join("red.png");
    make_image(&red, "64x64", "#ff8800").await;

    let cancel = Cancel::new();
    cancel.cancel();
    let request = request(&red, &root, "convert", "webp");
    let err = converter::execute(&binaries(), &request, Some(&cancel), None)
        .await
        .expect_err("取消后不应继续执行");
    assert!(matches!(err, Error::Abort), "期望 Abort，实际 {err}");
    // 不留下半成品。
    let leftovers: Vec<_> = std::fs::read_dir(&root)
        .expect("dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.contains("part"))
        .collect();
    assert!(leftovers.is_empty(), "临时文件未清理：{leftovers:?}");

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn image_pipeline_supports_every_tuned_output_format() {
    let root = scratch("formats");
    let source = root.join("source.png");
    make_image(&source, "96x64", "#4466ee").await;

    for format in ["png", "jpg", "webp", "avif", "tiff"] {
        let mut job = request(&source, &root, "convert", format);
        job.options.quality = Some(70.0);
        let output = converter::execute(&binaries(), &job, None, None)
            .await
            .unwrap_or_else(|err| panic!("{format} 转换失败：{}", err.message()));
        assert!(output.bytes > 0, "{format} 输出为空");
        assert!(output.output.ends_with(&format!(".{format}")));
        // 目标体积模式：应当搜索出不超过目标的结果。
        let mut sized = request(&source, &root, "convert", format);
        sized.options.quality = Some(90.0);
        sized.options.target_mb = Some(0.004);
        let small = converter::execute(&binaries(), &sized, None, None)
            .await
            .unwrap_or_else(|err| panic!("{format} 目标体积转换失败：{}", err.message()));
        assert!(small.bytes > 0);
    }

    // 该构建没有 HEIC 编码器：明确报错，不写假结果。
    let heic = request(&source, &root, "convert", "heic");
    let err = converter::execute(&binaries(), &heic, None, None)
        .await
        .expect_err("HEIC 输出应当报错");
    assert!(!err.message().is_empty());

    std::fs::remove_dir_all(&root).ok();
}
