// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.

//! Logging library.
//!
//! Logs all entries to the output pipe.
use std::collections::BTreeMap;

use tracing::Level;
use tracing_subscriber::Layer;

use crate::output;

pub struct CustomLayer {
    tx: output::MultiOutput,
}

impl CustomLayer {
    pub const fn new(tx: output::MultiOutput) -> Self {
        CustomLayer {
            // tx: Mutex::new(RefCell::new(None)),
            tx,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub target: String,
    pub name: String,
    pub level: Level,
    pub message: String,
}

macro_rules! log {
    ($level:expr, $message:expr) => {
        $crate::layer::raw_log($level, format!("{}:{}", file!(), line!()), $message)
    };
}

pub(crate) use log;

/// Create a log entry object.
pub fn raw_log(level: Level, name: impl Into<String>, message: impl Into<String>) -> LogEntry {
    LogEntry {
        target: "helmci".into(),
        name: name.into(),
        level,
        message: message.into(),
    }
}

impl<S> Layer<S> for CustomLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut fields = BTreeMap::new();
        let mut visitor = JsonVisitor(&mut fields);
        event.record(&mut visitor);

        // Output the event in JSON
        let entry = LogEntry {
            target: event.metadata().target().into(),
            name: event.metadata().name().into(),
            level: *event.metadata().level(),
            message: fields
                .get("message")
                .unwrap_or(&serde_json::Value::String("No Message".to_string()))
                .as_str()
                .unwrap_or("Invalid Message")
                .to_string(),
        };

        self.tx.try_send(output::Message::Log(entry));
    }
}

struct JsonVisitor<'a>(&'a mut BTreeMap<String, serde_json::Value>);

impl<'a> tracing::field::Visit for JsonVisitor<'a> {
    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        self.0.insert(
            field.name().to_string(),
            serde_json::json!(value.to_string()),
        );
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(
            field.name().to_string(),
            serde_json::json!(format!("{value:?}")),
        );
    }
}
