// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.

//! Logging library.
//!
//! Logs all entries to the output pipe.

#[derive(Clone, Copy, Debug)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Trace => write!(f, "TRACE"),
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warning => write!(f, "WARNING"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    // pub target: String,
    pub name: String,
    pub level: LogLevel,
    pub message: String,
}

macro_rules! log {
    ($level:expr_2021, $message:expr_2021) => {
        $crate::logging::raw_log($level, format!("{}:{}", file!(), line!()), $message)
    };
}

pub use log;

/// Create a log entry object.
pub fn raw_log(level: LogLevel, name: impl Into<String>, message: impl Into<String>) -> LogEntry {
    LogEntry {
        name: name.into(),
        level,
        message: message.into(),
    }
}
