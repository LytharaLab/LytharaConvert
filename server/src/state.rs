//! 队列与共享状态：单一队列所有者，串行执行，通过 SSE 广播。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU16;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::converter::{self, Cancel, JobRequest, Options, ProgressCb};
use crate::settings::{self, Settings};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub mode: String,
    pub source: String,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub format: String,
    pub status: String,
    pub progress: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub input_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frames: Option<u64>,
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub name: String,
    pub output_dir: String,
}

impl Job {
    pub fn request(&self) -> JobRequest {
        JobRequest {
            source: self.source.clone(),
            sources: self.sources.clone(),
            mode: self.mode.clone(),
            format: self.format.clone(),
            name: self.name.clone(),
            output_dir: self.output_dir.clone(),
            options: self.options.clone(),
        }
    }
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub jobs: Mutex<Vec<Job>>,
    pub cancels: Mutex<HashMap<String, Cancel>>,
    /// 正在运行的工作协程数量（并发上限来自设置）。
    pub workers_active: Mutex<usize>,
    /// 当前实际监听的端口（设置里改端口后要重启才生效）。
    pub port: AtomicU16,
    pub events: broadcast::Sender<String>,
    pub settings_file: PathBuf,
}

/// 按当前设置解析引擎：设置里的自定义路径优先，其次环境变量，最后 exe 旁的 binaries/。
pub fn binaries_for(settings: &Settings) -> converter::Binaries {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let ffmpeg = settings.ffmpeg_override();
    let ffprobe = settings.ffprobe_override();
    converter::binaries_with(exe_dir.as_deref(), ffmpeg.as_deref(), ffprobe.as_deref())
}

/// 不带设置的解析（供启动早期与测试使用）。
pub fn binaries() -> converter::Binaries {
    converter::binaries(
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .as_deref(),
    )
}

impl AppState {
    pub fn load() -> Arc<Self> {
        let settings_file = settings::settings_file();
        let loaded = settings::load(&settings_file, settings::default_output_dir());
        let loaded_port = loaded.port;
        let (events, _) = broadcast::channel(64);
        Arc::new(Self {
            settings: Mutex::new(loaded),
            jobs: Mutex::new(Vec::new()),
            cancels: Mutex::new(HashMap::new()),
            workers_active: Mutex::new(0),
            port: AtomicU16::new(loaded_port),
            events,
            settings_file,
        })
    }

    pub fn jobs_snapshot(&self) -> Vec<Job> {
        self.jobs.lock().map(|guard| guard.clone()).unwrap_or_default()
    }

    pub fn settings_snapshot(&self) -> Settings {
        self.settings.lock().map(|guard| guard.clone()).unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        settings::save(&self.settings_file, settings)
    }

    /// 广播队列状态：SSE 客户端收到后整体替换本地队列。
    pub fn publish(&self) {
        let payload = serde_json::json!({ "type": "queue", "jobs": self.jobs_snapshot() }).to_string();
        let _ = self.events.send(payload);
    }

    /// 按设置的并发数补齐工作协程；每个协程串行取任务，直到队列没有待处理项。
    pub fn kick_worker(self: &Arc<Self>) {
        let target = self
            .settings_snapshot()
            .concurrency
            .clamp(1, crate::settings::MAX_CONCURRENCY) as usize;
        let mut started = 0usize;
        {
            let mut guard = match self.workers_active.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            while *guard < target {
                *guard += 1;
                started += 1;
            }
        }
        for _ in 0..started {
            let app = Arc::clone(self);
            tokio::spawn(async move { worker_loop(app).await });
        }
    }

    pub fn cancel_all(&self) {
        if let Ok(guard) = self.cancels.lock() {
            for cancel in guard.values() {
                cancel.cancel();
            }
        }
    }
}

/// 一个工作协程：取待处理任务 → 执行 → 写回结果，直到队列里没有 pending。
async fn worker_loop(app: Arc<AppState>) {
    loop {
        let next = {
            let mut jobs = match app.jobs.lock() {
                Ok(guard) => guard,
                Err(_) => break,
            };
            match jobs.iter().position(|job| job.status == "pending") {
                Some(index) => {
                    jobs[index].status = "running".into();
                    jobs[index].progress = 0;
                    jobs[index].error = None;
                    Some(jobs[index].clone())
                }
                None => None,
            }
        };
        let Some(job) = next else { break };

        let cancel = Cancel::new();
        if let Ok(mut guard) = app.cancels.lock() {
            guard.insert(job.id.clone(), cancel.clone());
        }
        app.publish();

        let progress_app = Arc::clone(&app);
        let progress_id = job.id.clone();
        let on_progress: ProgressCb = Arc::new(move |value: f64| {
            let percent = (value * 100.0).round().clamp(0.0, 100.0) as u32;
            let changed = {
                let mut jobs = match progress_app.jobs.lock() {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
                match jobs.iter_mut().find(|item| item.id == progress_id) {
                    Some(target) if target.progress != percent => {
                        target.progress = percent;
                        true
                    }
                    Some(_) => false,
                    None => false,
                }
            };
            if changed {
                progress_app.publish();
            }
        });

        let engine = binaries_for(&app.settings_snapshot());
        let result = converter::execute(&engine, &job.request(), Some(&cancel), Some(on_progress)).await;

        if let Ok(mut guard) = app.cancels.lock() {
            guard.remove(&job.id);
        }
        if let Ok(mut jobs) = app.jobs.lock() {
            if let Some(target) = jobs.iter_mut().find(|item| item.id == job.id) {
                match result {
                    Ok(output) => {
                        target.status = "done".into();
                        target.progress = 100;
                        target.bytes = Some(output.bytes);
                        target.output = Some(output.output);
                        target.frames = output.frames;
                        target.error = None;
                    }
                    Err(err) => {
                        target.status = if err.is_abort() { "cancelled" } else { "error" }.into();
                        target.error = Some(err.message());
                    }
                }
            }
        }
        if !app.settings_snapshot().keep_uploads {
            cleanup_upload(&job);
        }
        app.publish();
    }

    {
        if let Ok(mut guard) = app.workers_active.lock() {
            *guard = guard.saturating_sub(1);
        }
    }
    // 收尾：排队期间又来了新任务的话，这里再踢一次。
    let has_pending = app
        .jobs
        .lock()
        .map(|jobs| jobs.iter().any(|job| job.status == "pending"))
        .unwrap_or(false);
    if has_pending {
        app.kick_worker();
    }
}

/// 拖放上传到 appdata/uploads 的文件用完即删；用户可在设置里保留它们。
fn cleanup_upload(job: &Job) {
    let uploads = crate::settings::uploads_dir();
    for source in std::iter::once(&job.source).chain(job.sources.iter()) {
        let path = Path::new(source);
        if path.starts_with(&uploads) {
            let _ = std::fs::remove_file(path);
        }
    }
}
