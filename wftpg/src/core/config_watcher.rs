//! 配置文件监听模块
//!
//! 使用 notify 库监听配置文件变化，实现自动重载

use crate::core::config_manager::ConfigManager;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// 配置文件防抖时间（毫秒）
const CONFIG_RELOAD_DEBOUNCE_MS: u64 = 500;
/// 每帧最大处理事件数
const MAX_EVENTS_PER_FRAME: usize = 5;

/// 配置文件监听器
pub struct ConfigWatcher {
    watcher: Option<RecommendedWatcher>,
    receiver: Option<Receiver<Result<Event, notify::Error>>>,
    config_path: PathBuf,
    config_manager: ConfigManager,
    needs_reload: bool,
    last_event_time: Option<std::time::Instant>,
}

impl ConfigWatcher {
    /// 创建新的配置监听器
    pub fn new(config_path: &Path, config_manager: ConfigManager) -> Self {
        let mut watcher = Self {
            watcher: None,
            receiver: None,
            config_path: config_path.to_path_buf(),
            config_manager,
            needs_reload: false,
            last_event_time: None,
        };

        watcher.init_watcher();
        watcher
    }

    /// 初始化文件监听器
    fn init_watcher(&mut self) {
        let (tx, rx) = mpsc::channel();
        let config_path = self.config_path.clone();

        let watcher_result = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                let _ = tx.send(res);
            },
            notify::Config::default().with_poll_interval(Duration::from_secs(2)),
        );

        match watcher_result {
            Ok(mut watcher) => {
                // 监听配置文件
                if config_path.exists() {
                    if let Err(e) = watcher.watch(&config_path, RecursiveMode::NonRecursive) {
                        tracing::error!("Failed to watch config file: {}", e);
                    } else {
                        tracing::info!("Config watcher started for: {:?}", config_path);
                    }
                } else {
                    // 如果文件不存在，监听父目录
                    if let Some(parent) = config_path.parent() {
                        if let Err(e) = watcher.watch(parent, RecursiveMode::NonRecursive) {
                            tracing::error!("Failed to watch config directory: {}", e);
                        } else {
                            tracing::info!("Config watcher started for directory: {:?}", parent);
                        }
                    }
                }

                self.watcher = Some(watcher);
                self.receiver = Some(rx);
            }
            Err(e) => {
                tracing::error!("Failed to create config watcher: {}", e);
            }
        }
    }

    /// 检查文件事件并重新加载配置
    pub fn check_and_reload(&mut self) -> bool {
        if let Some(rx) = &self.receiver {
            let mut event_count = 0;

            while let Ok(result) = rx.try_recv() {
                event_count += 1;
                if event_count > MAX_EVENTS_PER_FRAME {
                    break;
                }

                match result {
                    Ok(event) => {
                        // 检查是否是配置文件的变化
                        for path in &event.paths {
                            if path == &self.config_path
                                || (path.file_name().is_some_and(|n| n == "config.toml"))
                            {
                                let now = std::time::Instant::now();

                                // 防抖：在指定时间内只处理一次
                                if self.last_event_time.is_none_or(|t| {
                                    t.elapsed() >= Duration::from_millis(CONFIG_RELOAD_DEBOUNCE_MS)
                                }) {
                                    self.needs_reload = true;
                                    self.last_event_time = Some(now);
                                    tracing::info!("Config file changed: {:?}, will reload", path);
                                }
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Config watcher error: {}", e);
                    }
                }
            }
        }

        // 执行重新加载
        if self.needs_reload {
            self.needs_reload = false;
            return self.reload_config();
        }

        false
    }

    /// 重新加载配置
    fn reload_config(&self) -> bool {
        match self.config_manager.reload_from_file(&self.config_path) {
            Ok(_) => {
                tracing::info!("Configuration auto-reloaded successfully");
                true
            }
            Err(e) => {
                tracing::error!("Failed to auto-reload configuration: {}", e);
                false
            }
        }
    }

    /// 获取是否需要重新加载的状态
    pub fn needs_reload(&self) -> bool {
        self.needs_reload
    }
}
