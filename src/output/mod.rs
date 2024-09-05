// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.

//! Generic functionality for all output modules.
//!
//! A job may consist of one or more commands that need to be executed for successful completion of the job.
use anyhow::Result;
use async_trait::async_trait;
use semver::Version;
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::Instant};

use crate::{
    helm::{HelmResult, Installation},
    logging::LogEntry,
    Request,
};

pub mod slack;
pub mod text;
pub mod tui;

#[derive(Clone, Copy, Debug)]
pub enum JobSuccess {
    Completed,
    Skipped,
}

/// A message to the output module.
#[derive(Debug)]
pub enum Message {
    /// An job is to be skipped.
    SkippedJob(Arc<Installation>),
    /// A new job that is not going to be skipped.
    NewJob(Arc<Installation>),
    /// The version data for a job - only if outdated report requested.
    InstallationVersion(Arc<Installation>, Version, Version),
    /// The result of running a single command for a job.
    InstallationResult(Arc<HelmResult>),
    /// Notification that we started a job.
    StartedJob(Arc<Installation>, Instant),
    /// Notification that we finished a job.
    FinishedJob(Arc<Installation>, Result<JobSuccess, String>, Duration),

    /// A Log entry was logged.
    Log(LogEntry),

    /// This gets sent at very start.
    Start(Arc<Request>, Instant),
    /// This gets sent when all jobs declared finished, but UI should wait for socket to close before ending.
    FinishedAll(Result<(), String>, Duration),
}

#[derive(Clone)]
pub struct MultiOutput {
    tx: Vec<Sender>,
}

impl MultiOutput {
    pub fn new(tx: Vec<Sender>) -> Self {
        Self { tx }
    }

    pub async fn send(&self, msg: Message) {
        let msg = Arc::new(msg);
        for tx in &self.tx {
            tx.send(msg.clone()).await.unwrap_or_else(|err| {
                print!("Cannot send message to output pipe: {err}");
            });
        }
    }

    pub async fn send_log(&self, entry: LogEntry) {
        self.send(Message::Log(entry)).await;
    }
}

macro_rules! trace {
    ($tx:expr, $($t:tt)*) => {
        $tx.send_log($crate::logging::log!(
            $crate::logging::LogLevel::Trace,
            format!($($t)*)
        ))
    };
}
pub(crate) use trace;

macro_rules! debug {
    ($tx:expr, $($t:tt)*) => {
        $tx.send_log($crate::logging::log!(
            $crate::logging::LogLevel::Debug,
            format!($($t)*)
        ))
    };
}
pub(crate) use debug;

macro_rules! info {
    ($tx:expr, $($t:tt)*) => {
        $tx.send_log($crate::logging::log!(
            $crate::logging::LogLevel::Info,
            format!($($t)*)
        ))
    };
}
pub(crate) use info;

macro_rules! warning {
    ($tx:expr, $($t:tt)*) => {
        $tx.send_log($crate::logging::log!(
            $crate::logging::LogLevel::Warning,
            format!($($t)*)
        ))
    };
}
pub(crate) use warning;

macro_rules! error {
    ($tx:expr, $($t:tt)*) => {
        $tx.send_log($crate::logging::log!(
            $crate::logging::LogLevel::Error,
            format!($($t)*)
        ))
    };
}
pub(crate) use error;

pub type Sender = mpsc::Sender<Arc<Message>>;

/// Every output module should implement this trait.
#[async_trait]
pub trait Output {
    /// Wait for output to finish.
    async fn wait(&mut self) -> Result<()>;
}
