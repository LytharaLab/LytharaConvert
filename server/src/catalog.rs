//! 格式目录：扩展名归一化、分类与编解码器映射。
//! 对应 Electron 版本的 src/core/catalog.cjs。

use serde::Serialize;

pub const IMAGE: &[&str] = &[
    "png", "jpg", "webp", "avif", "gif", "apng", "tiff", "bmp", "ico", "heic", "jxl",
];
pub const VIDEO: &[&str] = &[
    "mp4", "mkv", "mov", "webm", "avi", "m4v", "ts", "flv", "wmv", "3gp",
];
pub const AUDIO: &[&str] = &[
    "mp3", "wav", "flac", "ogg", "opus", "aac", "m4a", "wma", "aiff", "alac",
];

/// 走「质量 + 目标体积」迭代的静态图片输出格式。
pub const TUNED_IMAGE_OUTPUT: &[&str] = &["png", "jpg", "webp", "avif", "tiff"];

pub const ANIMATED: &[&str] = &["gif", "apng"];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Image,
    Video,
    Audio,
    Other,
}

#[derive(Serialize)]
pub struct Formats {
    pub image: Vec<&'static str>,
    pub video: Vec<&'static str>,
    pub audio: Vec<&'static str>,
}

pub fn formats() -> Formats {
    Formats {
        image: IMAGE.to_vec(),
        video: VIDEO.to_vec(),
        audio: AUDIO.to_vec(),
    }
}

fn alias(ext: &str) -> &str {
    match ext {
        "jpeg" => "jpg",
        "tif" => "tiff",
        "wave" => "wav",
        "oga" => "ogg",
        "mpeg" => "mpg",
        other => other,
    }
}

pub fn normalize_ext(value: &str) -> String {
    let trimmed = value.trim().trim_start_matches('.').to_lowercase();
    alias(&trimmed).to_string()
}

/// 取文件扩展名（已归一化）。
pub fn extension(file: &str) -> String {
    let name = file
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(file);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => normalize_ext(ext),
        _ => String::new(),
    }
}

pub fn category(ext: &str) -> Category {
    let e = normalize_ext(ext);
    if IMAGE.contains(&e.as_str()) {
        Category::Image
    } else if VIDEO.contains(&e.as_str()) {
        Category::Video
    } else if AUDIO.contains(&e.as_str()) {
        Category::Audio
    } else {
        Category::Other
    }
}

/// 校验用户自定义扩展名，返回归一化结果。
pub fn safe_extension(value: &str) -> Result<String, String> {
    let e = normalize_ext(value);
    let ok = (1..=12).contains(&e.len()) && e.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if ok {
        Ok(e)
    } else {
        Err("扩展名须为 1–12 位英文字母或数字".into())
    }
}

pub fn is_animated(ext: &str) -> bool {
    ANIMATED.contains(&normalize_ext(ext).as_str())
}

pub fn audio_codec(ext: &str) -> Option<&'static str> {
    match normalize_ext(ext).as_str() {
        "mp3" => Some("libmp3lame"),
        "wav" => Some("pcm_s16le"),
        "flac" => Some("flac"),
        "ogg" => Some("libvorbis"),
        "opus" => Some("libopus"),
        "aac" => Some("aac"),
        "m4a" => Some("aac"),
        "wma" => Some("wmav2"),
        "aiff" => Some("pcm_s16be"),
        "alac" => Some("alac"),
        _ => None,
    }
}

/// 返回 (视频编码器, 音频编码器)。
pub fn video_codecs(ext: &str) -> Option<(&'static str, &'static str)> {
    match normalize_ext(ext).as_str() {
        "mp4" | "mkv" | "mov" => Some(("libx264", "aac")),
        "webm" => Some(("libvpx-vp9", "libopus")),
        "avi" => Some(("mpeg4", "libmp3lame")),
        "m4v" => Some(("libx264", "aac")),
        "ts" => Some(("mpeg2video", "mp2")),
        "flv" => Some(("flv", "libmp3lame")),
        "wmv" => Some(("wmv2", "wmav2")),
        "3gp" => Some(("h263", "aac")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_uses_last_segment_and_alias() {
        assert_eq!(extension(r"D:\media\clip.VIDEO.MP4"), "mp4");
        assert_eq!(extension("C:/pics/a.jpeg"), "jpg");
        assert_eq!(extension("no-extension"), "");
        assert_eq!(extension(r"D:\dir.with.dot\name"), "");
    }

    #[test]
    fn category_splits_image_video_audio() {
        assert_eq!(category("PNG"), Category::Image);
        assert_eq!(category(".tif"), Category::Image);
        assert_eq!(category("mkv"), Category::Video);
        assert_eq!(category("mp3"), Category::Audio);
        assert_eq!(category("exe"), Category::Other);
    }

    #[test]
    fn safe_extension_rejects_paths_and_symbols() {
        assert_eq!(safe_extension(".MP4").unwrap(), "mp4");
        assert!(safe_extension("a/b").is_err());
        assert!(safe_extension("").is_err());
        assert!(safe_extension("abcdefghijklmnop").is_err());
    }

    #[test]
    fn codec_tables_cover_documented_containers() {
        assert_eq!(video_codecs("mp4"), Some(("libx264", "aac")));
        assert_eq!(video_codecs("txt"), None);
        assert_eq!(audio_codec("flac"), Some("flac"));
    }
}
