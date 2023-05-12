// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.
use super::Message;
use super::Output;
use super::Sender;
use crate::duration::duration_string;
use crate::helm::HelmResult;
use crate::helm::Installation;
use crate::helm::InstallationId;
use crate::Task;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;
use tabled::settings::object::Columns;
use tabled::settings::object::Rows;
use tabled::settings::Alignment;
use tabled::settings::Modify;
use tabled::settings::Padding;
use tabled::settings::Style;
use tabled::Table;
use tabled::Tabled;
use tokio;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

pub struct TextOutput {
    thread: Option<JoinHandle<()>>,
}

#[derive(Copy, Clone)]
enum Status {
    Pending,
    InProgress,
    Complete,
    Skipped,
    Failed,
}

impl Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let char = match self {
            Status::Pending => '⚙',
            Status::InProgress => '☐',
            Status::Complete => '✅',
            Status::Skipped => '𝄩',
            Status::Failed => '❌',
        };
        f.write_char(char)
    }
}

struct DisplayableDuration(Option<Duration>);

impl Display for DisplayableDuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(duration) = self.0 {
            write!(f, "{}", duration_string(&duration))
        } else {
            write!(f, "-")
        }
    }
}

#[derive(Tabled)]
struct JobResult<'a> {
    status: Status,
    cluster: &'a str,
    namespace: &'a str,
    release: &'a str,
    duration: DisplayableDuration,
}

#[derive(Tabled)]
struct VersionResult<'a> {
    cluster: &'a str,
    namespace: &'a str,
    release: &'a str,
    #[tabled(rename = "our")]
    our_version: String,
    #[tabled(rename = "upstream")]
    upstream_version: String,
}

fn truncate(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        None => s,
        Some((idx, _)) => &s[..idx],
    }
}

fn results_to_string(state: &State) -> String {
    let mut data: Vec<JobResult> = state
        .jobs
        .iter()
        .filter_map(|installation| {
            state
                .results
                .get(&installation.id)
                .map(|result| to_job_result(result, installation))
        })
        .collect();

    let now = Instant::now();
    let duration = match (state.start_instant, &state.finished) {
        (None, None) => None,
        (_, Some((_, duration))) => Some(*duration),
        (Some(start_instant), None) => Some(now - start_instant),
    };

    let status = match &state.finished {
        None => Status::InProgress,
        Some((status, _)) => *status,
    };

    let total_title = match &state.finished {
        None => "RUNNING",
        Some(_) => "TOTAL",
    };

    data.push(JobResult {
        status,
        cluster: "",
        release: total_title,
        namespace: "",
        duration: DisplayableDuration(duration),
    });

    Table::new(data)
        .with(Style::markdown())
        .with(Modify::new(Rows::new(..)).with(Alignment::left()))
        .with(Modify::new(Columns::first()).with(Alignment::center()))
        .with(Modify::new(Rows::last()).with(Padding::new(1, 1, 1, 0).fill(' ', ' ', '-', ' ')))
        .to_string()
}

fn to_job_result<'a>(
    result: &(Status, Option<Instant>, Option<Duration>),
    installation: &'a Installation,
) -> JobResult<'a> {
    let duration = match (result.1, result.2) {
        (None, None) => None,
        (_, Some(duration)) => Some(duration),
        (Some(start_instant), None) => Some(Instant::now() - start_instant),
    };
    JobResult {
        status: result.0,
        cluster: &installation.cluster_name,
        namespace: &installation.namespace,
        release: truncate(&installation.name, 25),
        duration: DisplayableDuration(duration),
    }
}

fn versions_to_string(state: &State) -> String {
    let data: Vec<VersionResult> = state
        .jobs
        .iter()
        .filter_map(|installation| {
            if let Some((our_version, upstream_version)) = state.versions.get(&installation.id) {
                Some(VersionResult {
                    cluster: &installation.cluster_name,
                    namespace: &installation.namespace,
                    release: truncate(&installation.name, 25),
                    our_version: our_version.clone(),
                    upstream_version: upstream_version.clone(),
                })
            } else {
                None
            }
        })
        .collect();

    Table::new(data)
        .with(Style::markdown())
        .with(Modify::new(Rows::new(..)).with(Alignment::left()))
        .to_string()
}

fn update_results(state: &State, finished: bool) {
    if finished {
        let results = results_to_string(state);
        println!("\n\n{results}");

        if !state.versions.is_empty() {
            let versions = versions_to_string(state);
            println!("\n\n{versions}");
        }
    };
}

struct GitlabSection {
    name: String,
    title: String,
    duration: Duration,
}

impl GitlabSection {
    fn start(&self) {
        #[allow(clippy::cast_sign_loss)]
        let start = chrono::offset::Utc::now().timestamp() as u64 - self.duration.as_secs();
        println!(
            "\x1B[0Ksection_start:{}:{}[collapsed=true]\r\x1B[0K{}",
            start, self.name, self.title
        );
    }
    fn stop(&self) {
        println!(
            "\x1B[0Ksection_end:{}:{}[collapsed=true]\r\x1B[0K",
            chrono::offset::Utc::now().timestamp(),
            self.name,
        );
    }
}

fn process_message(msg: &Arc<Message>, state: &mut State) {
    match msg.as_ref() {
        Message::InstallationResult(hr) => {
            let HelmResult {
                command,
                result,
                installation,
            } = hr.as_ref();
            let result_str = hr.result_line();

            let s = GitlabSection {
                name: format!("{}_{command:?}", installation.name),
                title: format!("{} {command} {result_str}", installation.name),
                duration: hr.duration(),
            };
            s.start();
            println!("------------------------------------------");
            match result {
                Ok(success) => print!("{success}"),
                Err(err) => print!("{err}"),
            }
            s.stop();
        }
        Message::Log(entry) => println!(
            "{} {} {} {}",
            entry.level, entry.target, entry.name, entry.message
        ),
        Message::SkippedJob(installation) => {
            state
                .results
                .insert(installation.id, (Status::Skipped, None, None));
            state.jobs.push(installation.clone());
        }
        Message::InstallationVersion(installation, our_version, upstream_version) => {
            if our_version != upstream_version {
                state.versions.insert(
                    installation.id,
                    (our_version.clone(), upstream_version.clone()),
                );
            }
        }
        Message::NewJob(installation) => {
            state
                .results
                .insert(installation.id, (Status::Pending, None, None));
            state.jobs.push(installation.clone());
        }
        Message::StartedJob(installation, start_instant) => {
            state.results.insert(
                installation.id,
                (Status::InProgress, Some(*start_instant), None),
            );
        }
        Message::FinishedJob(installation, result, duration) => {
            let status = match result {
                Ok(_) => Status::Complete,
                Err(_) => Status::Failed,
            };
            state
                .results
                .insert(installation.id, (status, None, Some(*duration)));
        }
        Message::FinishedAll(rc, duration) => {
            let status = match rc {
                Ok(_) => Status::Complete,
                Err(_) => Status::Failed,
            };
            state.finished = Some((status, *duration));
        }
        Message::Start(task, start_instant) => {
            state.task = Some(*task);
            state.start_instant = Some(*start_instant);
        }
    }
}

#[derive(Clone)]
struct State {
    task: Option<Task>,
    start_instant: Option<Instant>,
    results: HashMap<InstallationId, (Status, Option<Instant>, Option<Duration>)>,
    versions: HashMap<InstallationId, (String, String)>,
    jobs: Vec<Arc<Installation>>,
    finished: Option<(Status, Duration)>,
}
pub fn start() -> (TextOutput, Sender) {
    let (tx, mut rx) = mpsc::channel(50);

    let thread = tokio::spawn(async move {
        let mut state = State {
            task: None,
            start_instant: None,
            results: HashMap::new(),
            versions: HashMap::new(),
            jobs: Vec::new(),
            finished: None,
        };
        while let Some(msg) = rx.recv().await {
            process_message(&msg, &mut state);
        }
        update_results(&state, true);
    });

    (
        TextOutput {
            thread: Some(thread),
        },
        tx,
    )
}

#[async_trait]
impl Output for TextOutput {
    async fn wait(&mut self) -> Result<()> {
        if let Some(thread) = self.thread.take() {
            thread.await?;
        }
        Ok(())
    }
}
