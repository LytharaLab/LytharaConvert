//! 设置持久化：`<exe 同级>/appdata/settings.json`。
//!
//! 所有可调项都带默认值，字段缺失或非法一律退回默认，保证旧配置文件仍能启动。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const THEMES: [&str; 3] = ["system", "light", "dark"];
pub const DEFAULT_PORT: u16 = 8765;
pub const MIN_PORT: u16 = 1024;
pub const MAX_CONCURRENCY: u32 = 8;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 转换结果保存位置。
    pub output_dir: String,
    /// 界面主题：system / light / dark。
    pub theme: String,
    /// 服务端口（改动需重启服务生效）。
    pub port: u16,
    /// 启动时自动打开浏览器。
    pub open_browser: bool,
    /// 自定义 ffmpeg 路径；空 = 自动探测 exe 旁的 binaries。
    pub ffmpeg_path: String,
    /// 自定义 ffprobe 路径；空 = 自动探测。
    pub ffprobe_path: String,
    /// 同时处理的任务数（1–8）。
    pub concurrency: u32,
    /// 任务结束后发浏览器通知。
    pub notify_on_finish: bool,
    /// 保留拖放上传到 appdata/uploads 的原始文件。
    pub keep_uploads: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: String::new(),
            theme: "system".into(),
            port: DEFAULT_PORT,
            open_browser: true,
            ffmpeg_path: String::new(),
            ffprobe_path: String::new(),
            concurrency: 1,
            notify_on_finish: false,
            keep_uploads: false,
        }
    }
}

impl Settings {
    pub fn new(default_output_dir: PathBuf) -> Self {
        Self {
            output_dir: default_output_dir.to_string_lossy().to_string(),
            ..Default::default()
        }
    }

    pub fn theme_value(&self) -> &str {
        if THEMES.contains(&self.theme.as_str()) {
            &self.theme
        } else {
            "system"
        }
    }

    /// 把非法值掰回可用范围；返回是否发生了修改。
    pub fn sanitize(&mut self) -> bool {
        let before = self.clone();
        if !THEMES.contains(&self.theme.as_str()) {
            self.theme = "system".into();
        }
        if self.port < MIN_PORT {
            self.port = DEFAULT_PORT;
        }
        if self.concurrency == 0 || self.concurrency > MAX_CONCURRENCY {
            self.concurrency = self.concurrency.clamp(1, MAX_CONCURRENCY);
        }
        self.ffmpeg_path = self.ffmpeg_path.trim().to_string();
        self.ffprobe_path = self.ffprobe_path.trim().to_string();
        self.output_dir = self.output_dir.trim().to_string();
        self.theme != before.theme
            || self.port != before.port
            || self.concurrency != before.concurrency
            || self.ffmpeg_path != before.ffmpeg_path
            || self.ffprobe_path != before.ffprobe_path
            || self.output_dir != before.output_dir
    }

    /// 引擎的显式覆盖（空字符串表示没配）。
    pub fn ffmpeg_override(&self) -> Option<PathBuf> {
        non_empty(&self.ffmpeg_path)
    }

    pub fn ffprobe_override(&self) -> Option<PathBuf> {
        non_empty(&self.ffprobe_path)
    }
}

fn non_empty(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// 默认输出目录：`%USERPROFILE%\Downloads\Lythara Convert`。
pub fn default_output_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("Downloads").join("Lythara Convert")
}

/// 应用数据目录：**exe 同级的 appdata/**（便携，绿色版友好）。
/// 该目录不可写时（比如装在 Program Files 下）退回 `%APPDATA%\Lythara Convert`。
pub fn data_dir() -> PathBuf {
    static CACHE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    CACHE.get_or_init(compute_data_dir).clone()
}

fn compute_data_dir() -> PathBuf {
    if let Some(parent) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        let candidate = parent.join("appdata");
        if writable(&candidate) {
            return candidate;
        }
    }
    let fallback = std::env::var("APPDATA")
        .map(|dir| PathBuf::from(dir).join("Lythara Convert"))
        .unwrap_or_else(|_| PathBuf::from("appdata"));
    let _ = std::fs::create_dir_all(&fallback);
    fallback
}

fn writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write-probe");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 设置文件：`<exe 同级>/appdata/settings.json`。
pub fn settings_file() -> PathBuf {
    data_dir().join("settings.json")
}

/// 拖放上传的落盘目录：`<exe 同级>/appdata/uploads/`。
pub fn uploads_dir() -> PathBuf {
    data_dir().join("uploads")
}

/// 转换过程的中间目录：`<exe 同级>/appdata/tmp/`。
pub fn temp_dir() -> PathBuf {
    data_dir().join("tmp")
}

pub fn load(path: &Path, default_output_dir: PathBuf) -> Settings {
    let fallback = Settings::new(default_output_dir.clone());
    let mut settings = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Settings>(&text).unwrap_or(fallback),
        Err(_) => fallback,
    };
    settings.sanitize();
    if settings.output_dir.trim().is_empty() {
        settings.output_dir = default_output_dir.to_string_lossy().to_string();
    }
    settings
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|err| err.to_string())?;
    std::fs::write(path, text).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-scratch")
            .join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn defaults_are_usable() {
        let settings = Settings::default();
        assert_eq!(settings.port, DEFAULT_PORT);
        assert_eq!(settings.concurrency, 1);
        assert!(settings.open_browser);
        assert!(settings.ffmpeg_override().is_none());
    }

    #[test]
    fn missing_file_or_broken_json_falls_back() {
        let dir = scratch("settings-fallback");
        let file = dir.join("settings.json");
        let _ = std::fs::remove_file(&file);

        let defaults = load(&file, PathBuf::from(r"C:\downloads\Lythara Convert"));
        assert_eq!(defaults.theme, "system");
        assert!(defaults.output_dir.ends_with("Lythara Convert"));

        std::fs::write(&file, "{ not json").unwrap();
        assert_eq!(load(&file, PathBuf::from("x")).output_dir, "x");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn partial_file_keeps_defaults_and_sanitizes() {
        let dir = scratch("settings-partial");
        let file = dir.join("settings.json");
        // 只有两个字段：其余必须落到默认值。
        std::fs::write(&file, r#"{"outputDir":"D:\\out","theme":"neon","port":80,"concurrency":99}"#).unwrap();
        let loaded = load(&file, PathBuf::from("ignored"));
        assert_eq!(loaded.output_dir, r"D:\out");
        assert_eq!(loaded.theme, "system");
        assert_eq!(loaded.port, DEFAULT_PORT, "低于 1024 的端口应退回默认");
        assert_eq!(loaded.concurrency, MAX_CONCURRENCY);
        assert!(loaded.open_browser, "缺省字段应保留默认值");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn round_trips_every_field() {
        let dir = scratch("settings-roundtrip");
        let file = dir.join("nested").join("settings.json");
        let mut settings = Settings::new(PathBuf::from(r"D:\media"));
        settings.theme = "dark".into();
        settings.port = 9000;
        settings.open_browser = false;
        settings.ffmpeg_path = r" C:\tools\ffmpeg.exe ".into();
        settings.ffprobe_path = r"C:\tools\ffprobe.exe".into();
        settings.concurrency = 3;
        settings.notify_on_finish = true;
        settings.keep_uploads = true;
        save(&file, &settings).unwrap();

        let loaded = load(&file, PathBuf::from("ignored"));
        assert_eq!(loaded.theme, "dark");
        assert_eq!(loaded.port, 9000);
        assert!(!loaded.open_browser);
        assert_eq!(loaded.ffmpeg_override().unwrap(), PathBuf::from(r"C:\tools\ffmpeg.exe"));
        assert_eq!(loaded.ffprobe_override().unwrap(), PathBuf::from(r"C:\tools\ffprobe.exe"));
        assert_eq!(loaded.concurrency, 3);
        assert!(loaded.notify_on_finish);
        assert!(loaded.keep_uploads);
        assert_eq!(loaded.output_dir, r"D:\media");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
