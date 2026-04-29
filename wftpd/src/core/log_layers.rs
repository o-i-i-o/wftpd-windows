//! Tracing log layers

use chrono::Local;
use tracing_subscriber::Layer;

use super::log_types::{LogBuffer, LogEntry, LogFields, LogLevel};

pub struct SystemLogLayer {
    buffer: LogBuffer<LogEntry>,
}

impl SystemLogLayer {
    pub fn new(buffer: LogBuffer<LogEntry>) -> Self {
        Self { buffer }
    }
}

impl<S> Layer<S> for SystemLogLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let target = metadata.target();

        if target.starts_with("file_op") {
            return;
        }

        let level = *metadata.level();
        let log_level = LogLevel::from_tracing_level(level);

        let mut visitor = SystemFieldVisitor::new();
        event.record(&mut visitor);

        let entry = LogEntry {
            timestamp: Local::now(),
            level: log_level,
            fields: LogFields {
                message: visitor.message.unwrap_or_default(),
                client_ip: visitor.client_ip,
                username: visitor.username,
                action: visitor.action,
                protocol: visitor.protocol,
                operation: None,
                file_path: None,
                file_size: None,
                success: None,
            },
        };

        self.buffer.push(entry);
    }
}

pub struct FileOpLogLayer {
    buffer: LogBuffer<LogEntry>,
}

impl FileOpLogLayer {
    pub fn new(buffer: LogBuffer<LogEntry>) -> Self {
        Self { buffer }
    }
}

impl<S> Layer<S> for FileOpLogLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let target = metadata.target();

        if !target.starts_with("file_op") {
            return;
        }

        let level = *metadata.level();
        let log_level = LogLevel::from_tracing_level(level);

        let mut visitor = FileOpFieldVisitor::new();
        event.record(&mut visitor);

        let entry = LogEntry {
            timestamp: Local::now(),
            level: log_level,
            fields: LogFields {
                message: visitor.message.unwrap_or_default(),
                client_ip: visitor.client_ip.clone(),
                username: visitor.username.clone(),
                action: None,
                protocol: visitor.protocol.clone(),
                operation: visitor.operation.clone(),
                file_path: visitor.file_path.clone(),
                file_size: visitor.file_size,
                success: visitor.success,
            },
        };

        self.buffer.push(entry);
    }
}

pub struct SystemFieldVisitor {
    pub message: Option<String>,
    pub client_ip: Option<String>,
    pub username: Option<String>,
    pub action: Option<String>,
    pub protocol: Option<String>,
}

impl SystemFieldVisitor {
    pub fn new() -> Self {
        Self {
            message: None,
            client_ip: None,
            username: None,
            action: None,
            protocol: None,
        }
    }
}

impl Default for SystemFieldVisitor {
    fn default() -> Self {
        Self::new()
    }
}

impl tracing::field::Visit for SystemFieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => self.message = Some(value.to_string()),
            "client_ip" => self.client_ip = Some(value.to_string()),
            "username" => self.username = Some(value.to_string()),
            "action" => self.action = Some(value.to_string()),
            "protocol" => self.protocol = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_i64(&mut self, _field: &tracing::field::Field, _value: i64) {}
    fn record_u64(&mut self, _field: &tracing::field::Field, _value: u64) {}
    fn record_bool(&mut self, _field: &tracing::field::Field, _value: bool) {}
}

pub struct FileOpFieldVisitor {
    pub message: Option<String>,
    pub client_ip: Option<String>,
    pub username: Option<String>,
    pub operation: Option<String>,
    pub file_path: Option<String>,
    pub file_size: Option<u64>,
    pub protocol: Option<String>,
    pub success: Option<bool>,
}

impl FileOpFieldVisitor {
    pub fn new() -> Self {
        Self {
            message: None,
            client_ip: None,
            username: None,
            operation: None,
            file_path: None,
            file_size: None,
            protocol: None,
            success: None,
        }
    }
}

impl Default for FileOpFieldVisitor {
    fn default() -> Self {
        Self::new()
    }
}

impl tracing::field::Visit for FileOpFieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => self.message = Some(value.to_string()),
            "client_ip" => self.client_ip = Some(value.to_string()),
            "username" => self.username = Some(value.to_string()),
            "operation" => self.operation = Some(value.to_string()),
            "file_path" => self.file_path = Some(value.to_string()),
            "protocol" => self.protocol = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == "file_size" {
            self.file_size = Some(value);
        }
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if field.name() == "success" {
            self.success = Some(value);
        }
    }

    fn record_i64(&mut self, _field: &tracing::field::Field, _value: i64) {}
}
