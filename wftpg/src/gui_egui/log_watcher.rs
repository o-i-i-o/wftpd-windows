use crate::core::logger::LogEntry;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const MAX_DISPLAY_LOGS: usize = 500;
const INITIAL_FETCH_COUNT: usize = 100;

pub struct LogFileWatcher {
    log_dir: PathBuf,
    file_prefix: &'static str,
    filter_fn: fn(&LogEntry) -> bool,
    logs: VecDeque<LogEntry>,
    last_file_pos: u64,
    current_log_file: Option<PathBuf>,
    watcher: Option<RecommendedWatcher>,
    rx: Option<Receiver<Result<Event, notify::Error>>>,
    needs_refresh: bool,
    last_event_time: Option<Instant>,
    last_refresh_time: Option<Instant>,
}

impl LogFileWatcher {
    pub fn new(
        log_dir: PathBuf,
        file_prefix: &'static str,
        filter_fn: fn(&LogEntry) -> bool,
    ) -> Self {
        Self {
            log_dir,
            file_prefix,
            filter_fn,
            logs: VecDeque::with_capacity(MAX_DISPLAY_LOGS),
            last_file_pos: 0,
            current_log_file: None,
            watcher: None,
            rx: None,
            needs_refresh: false,
            last_event_time: None,
            last_refresh_time: None,
        }
    }

    pub fn init(&mut self) {
        self.init_watcher();
        self.full_reload();
    }

    fn init_watcher(&mut self) {
        let (tx, rx) = mpsc::channel();

        let watcher_result = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Err(e) = tx.send(res) {
                    tracing::debug!("Log watcher channel send error: {}", e);
                }
            },
            notify::Config::default().with_poll_interval(Duration::from_millis(500)),
        );

        match watcher_result {
            Ok(mut watcher) => {
                self.try_watch_dir(&mut watcher);
                self.watcher = Some(watcher);
                self.rx = Some(rx);
            }
            Err(e) => {
                tracing::error!("Failed to create log watcher: {}", e);
            }
        }
    }

    fn try_watch_dir(&mut self, watcher: &mut RecommendedWatcher) {
        if self.log_dir.exists() {
            if let Err(e) = watcher.watch(&self.log_dir, RecursiveMode::NonRecursive) {
                tracing::warn!("Failed to watch log directory: {}", e);
            }
        }
    }

    pub fn check_events(&mut self, ctx: &egui::Context) {
        if !self.log_dir.exists() {
            if let Some(watcher) = &mut self.watcher {
                if let Err(e) = watcher.watch(&self.log_dir, RecursiveMode::NonRecursive) {
                    tracing::warn!("Failed to watch log directory: {}", e);
                }
            }
            return;
        }

        let Some(rx) = &self.rx else {
            return;
        };

        let mut event_count = 0;
        while let Ok(result) = rx.try_recv() {
            event_count += 1;
            if event_count > 10 {
                break;
            }
            match result {
                Ok(event) => {
                    for path in &event.paths {
                        if path.extension().is_some_and(|ext| ext == "log")
                            && path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .is_some_and(|n| n.starts_with(self.file_prefix))
                        {
                            if self
                                .last_event_time
                                .is_none_or(|t| t.elapsed() >= Duration::from_millis(100))
                            {
                                self.needs_refresh = true;
                                self.last_event_time = Some(Instant::now());
                                ctx.request_repaint();
                            }
                            break;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Log watcher error: {}", e);
                }
            }
        }
    }

    pub fn process_refresh(&mut self) {
        if !self.needs_refresh {
            return;
        }
        self.needs_refresh = false;

        if self.detect_log_rotation() {
            self.full_reload();
            return;
        }

        self.incremental_read();
    }

    fn detect_log_rotation(&mut self) -> bool {
        let latest = self.find_latest_log_file();
        match (&self.current_log_file, &latest) {
            (Some(current), Some(latest)) if current != latest => {
                tracing::debug!("Log rotation detected: {:?} -> {:?}", current, latest);
                return true;
            }
            (None, Some(_)) => return true,
            _ => {}
        }
        false
    }

    fn find_latest_log_file(&self) -> Option<PathBuf> {
        let entries = fs::read_dir(&self.log_dir).ok()?;
        let mut log_files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(self.file_prefix) && name.ends_with(".log"))
            })
            .collect();

        log_files.sort_by(|a, b| {
            let a_time = a.metadata().and_then(|m| m.modified()).ok();
            let b_time = b.metadata().and_then(|m| m.modified()).ok();
            b_time.cmp(&a_time)
        });

        log_files.first().map(|e| e.path())
    }

    pub fn full_reload(&mut self) {
        self.logs.clear();

        if let Some(latest_path) = self.find_latest_log_file() {
            self.current_log_file = Some(latest_path.clone());

            if let Ok(file) = File::open(&latest_path) {
                let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);

                let reader = BufReader::new(file);
                let all_lines: Vec<_> = reader.lines().filter_map(Result::ok).collect();

                let start = all_lines.len().saturating_sub(INITIAL_FETCH_COUNT);
                for line in all_lines.into_iter().skip(start) {
                    if let Ok(entry) = serde_json::from_str::<LogEntry>(&line)
                        && (self.filter_fn)(&entry)
                    {
                        if self.logs.len() >= MAX_DISPLAY_LOGS {
                            self.logs.pop_front();
                        }
                        self.logs.push_back(entry);
                    }
                }

                self.last_file_pos = file_size;
            }
        }

        self.last_refresh_time = Some(Instant::now());
    }

    fn incremental_read(&mut self) {
        let Some(current_file) = &self.current_log_file else {
            return;
        };

        if !current_file.exists() {
            self.full_reload();
            return;
        }

        let Ok(file) = File::open(current_file) else {
            return;
        };
        let Ok(metadata) = file.metadata() else {
            return;
        };

        let current_size = metadata.len();

        if current_size < self.last_file_pos {
            self.full_reload();
            return;
        }

        if current_size == self.last_file_pos {
            return;
        }

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(self.last_file_pos)).is_err() {
            return;
        }

        let mut new_entries = Vec::new();

        for line in reader.lines().filter_map(Result::ok) {
            if let Ok(entry) = serde_json::from_str::<LogEntry>(&line)
                && (self.filter_fn)(&entry)
            {
                new_entries.push(entry);
            }
        }

        self.last_file_pos = current_size;

        if !new_entries.is_empty() {
            for entry in new_entries {
                if self.logs.len() >= MAX_DISPLAY_LOGS {
                    self.logs.pop_front();
                }
                self.logs.push_back(entry);
            }
            self.last_refresh_time = Some(Instant::now());
        }
    }

    pub fn logs(&self) -> &VecDeque<LogEntry> {
        &self.logs
    }

    pub fn last_refresh_time(&self) -> Option<Instant> {
        self.last_refresh_time
    }

    pub fn request_refresh(&mut self) {
        self.full_reload();
    }
}
