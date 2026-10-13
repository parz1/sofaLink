//! Asynchronous Windows adapter. Only core selects operation order and allowed commands.
use sofalink_core::*;
use std::os::windows::process::CommandExt;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[derive(Clone)]
pub struct Config {
    pub root: PathBuf,
    pub shell: PathBuf,
    pub device: String,
    pub python: PathBuf,
}
pub struct Binding {
    pub game: WindowRef,
    pub monitor: u64,
}
pub struct Completion {
    pub effect: SpaceEffect,
    pub resources: SpaceResources,
    pub error: Option<String>,
    pub binding: Option<Binding>,
}
pub struct SpaceBackend {
    config: Config,
    tx: Sender<Completion>,
    rx: Receiver<Completion>,
    health_tx: Sender<u64>,
    health_rx: Receiver<u64>,
}
impl SpaceBackend {
    pub fn new(config: Config) -> Self {
        let (tx, rx) = mpsc::channel();
        let (health_tx, health_rx) = mpsc::channel();
        Self {
            config,
            tx,
            rx,
            health_tx,
            health_rx,
        }
    }
    pub fn execute(&self, effect: SpaceEffect) {
        let config = self.config.clone();
        let tx = self.tx.clone();
        let health_tx = self.health_tx.clone();
        let owner = std::process::id();
        thread::spawn(move || {
            let result_path = config.root.join(format!(
                "local/game-space/operation-{owner}-{}.json",
                effect.request
            ));
            let operation = serde_json::to_value(effect.operation)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned();
            let result = (|| -> Result<Completion, String> {
                let output = std::process::Command::new(&config.shell)
                    .creation_flags(0x08000000)
                    .args(["-NoProfile", "-File"])
                    .arg(
                        config
                            .root
                            .join("experiments/game-space/space-operation.ps1"),
                    )
                    .args(["-Action", &operation, "-ResultFile"])
                    .arg(&result_path)
                    .args(["-Device", &config.device, "-Python"])
                    .arg(&config.python)
                    .arg("-Shell")
                    .arg(&config.shell)
                    .args(["-OwnerPid", &owner.to_string()])
                    .output()
                    .map_err(|e| e.to_string())?;
                if !output.status.success() {
                    return Err("系统操作适配器退出，请检查本机日志".into());
                }
                let raw = std::fs::read_to_string(&result_path).map_err(|e| e.to_string())?;
                let value: serde_json::Value =
                    serde_json::from_str(raw.trim_start_matches('\u{feff}'))
                        .map_err(|e| e.to_string())?;
                let resources = serde_json::from_value(value["resources"].clone())
                    .map_err(|e| e.to_string())?;
                let mut error = value["error"].as_str().map(str::to_owned);
                let binding = value["binding"]["game_pid"]
                    .as_u64()
                    .and_then(|pid| sofalink_input::focus_adapter::game_window(pid as u32))
                    .zip(value["binding"]["monitor"].as_u64())
                    .map(|(game, monitor)| Binding { game, monitor });
                if effect.operation == SpaceOperation::StartMedia
                    && error.is_none()
                    && !binding
                        .as_ref()
                        .is_some_and(|b| sofalink_input::focus_adapter::in_space(b.game, b.monitor))
                {
                    error = Some("游戏所在串流屏幕未确认".into());
                }
                if effect.operation == SpaceOperation::StartMedia && error.is_none() {
                    let path = value["binding"]["health_file"]
                        .as_str()
                        .ok_or("媒体健康记录缺失")?;
                    let path = PathBuf::from(path);
                    let sender = health_tx.clone();
                    thread::spawn(move || {
                        let mut failures = 0;
                        loop {
                            thread::sleep(Duration::from_secs(1));
                            let healthy = std::fs::read_to_string(&path)
                                .ok()
                                .and_then(|raw| {
                                    serde_json::from_str::<serde_json::Value>(
                                        raw.trim_start_matches('\u{feff}'),
                                    )
                                    .ok()
                                })
                                .is_some_and(|v| {
                                    let now = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis()
                                        as u64;
                                    v["state"] == "running"
                                        && v["updated_ms"]
                                            .as_u64()
                                            .is_some_and(|t| now.saturating_sub(t) < 6000)
                                });
                            if healthy {
                                failures = 0;
                            } else {
                                failures += 1;
                            }
                            if failures >= 2 {
                                let _ = sender.send(effect.request);
                                break;
                            }
                        }
                    });
                }
                Ok(Completion {
                    effect,
                    resources,
                    error,
                    binding,
                })
            })();
            let event = result.unwrap_or_else(|error| Completion {
                effect,
                resources: SpaceResources::default(),
                error: Some(error),
                binding: None,
            });
            let _ = tx.send(event);
        });
    }
    pub fn poll(&self) -> Vec<Completion> {
        self.rx.try_iter().collect()
    }
    pub fn lost_sessions(&self) -> Vec<u64> {
        self.health_rx.try_iter().collect()
    }
}
