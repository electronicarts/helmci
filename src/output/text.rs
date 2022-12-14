// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.
use super::Message;
use super::Output;
use super::Sender;
use crate::config::AnnouncePolicy;
use crate::duration::duration_string;
use crate::helm::HelmResult;
use crate::helm::Installation;
use crate::helm::InstallationId;
use crate::Task;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use slack_morphism::prelude::*;
use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::Write;
use std::time::Duration;
use tabled::object::Columns;
use tabled::object::Rows;
use tabled::Alignment;
use tabled::ModifyObject;
use tabled::Padding;
use tabled::Style;
use tabled::Table;
use tabled::Tabled;
use tokio;
use tokio::select;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tokio::time::Instant;
use tracing::error;

pub struct TextOutput {
    tx: Option<Sender>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Copy, Clone)]
enum Status {
    Pending,
    InProgess,
    Complete,
    Skipped,
    Failed,
}

impl Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let char = match self {
            Status::Pending => '⚙',
            Status::InProgess => '☐',
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
        None => Status::InProgess,
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
        .with(Rows::new(..).modify().with(Alignment::left()))
        .with(Columns::first().modify().with(Alignment::center()))
        .with(
            Rows::last()
                .modify()
                .with(Padding::new(1, 1, 1, 0).set_fill(' ', ' ', '-', ' ')),
        )
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
        .with(Rows::new(..).modify().with(Alignment::left()))
        .to_string()
}

pub fn config_env_var(name: &str) -> Result<String, Error> {
    std::env::var(name).map_err(|e| anyhow::anyhow!("{}: {}", name, e))
}

#[derive(Clone)]
struct SlackState {
    token: SlackApiToken,
    slack_channel: String,
    announce_slack_channel: String,
    ts: Option<SlackTs>,
}

impl SlackState {
    fn new() -> Result<SlackState> {
        let slack_channel: String = config_env_var("SLACK_CHANNEL")?;
        let token_value: SlackApiTokenValue = config_env_var("SLACK_API_TOKEN")?.into();
        let token: SlackApiToken = SlackApiToken::new(token_value);

        if slack_channel.is_empty() {
            return Err(anyhow::anyhow!("SLACK_CHANNEL is empty"));
        }

        if slack_channel.is_empty() {
            return Err(anyhow::anyhow!("SLACK_API_TOKEN is empty"));
        }

        let announce_slack_channel: String =
            config_env_var("SLACK_CHANNEL_ANNOUNCE").unwrap_or_else(|_| slack_channel.clone());

        Ok(SlackState {
            token,
            slack_channel,
            announce_slack_channel,
            ts: None,
        })
    }

    async fn update_slack<'a>(&mut self, state: &State) -> Result<()> {
        let client = SlackClient::new(SlackClientHyperConnector::new());
        let session = client.open_session(&self.token);

        let title = slack_title(state);

        let mut installation_blocks = get_installation_blocks(state, &title);
        let mut outdated_blocks = get_outdated_blocks(state);

        let mut blocks = vec![];
        blocks.append(&mut installation_blocks);
        blocks.append(&mut outdated_blocks);

        let content = SlackMessageContent::new()
            .with_blocks(blocks)
            .with_text(title);

        if let Some(ts) = &self.ts {
            let update_req = SlackApiChatUpdateRequest::new(
                self.slack_channel.clone().into(),
                content,
                ts.clone(),
            );
            let update_resp = session.chat_update(&update_req).await?;
            self.ts = Some(update_resp.ts);
        } else {
            let post_chat_req =
                SlackApiChatPostMessageRequest::new(self.slack_channel.clone().into(), content);
            let post_chat_resp = session.chat_post_message(&post_chat_req).await?;
            self.ts = Some(post_chat_resp.ts);
        }

        Ok(())
    }

    async fn send_finished(&self, state: &State) -> Result<()> {
        #[allow(clippy::match_same_arms)]
        let data = state
            .jobs
            .iter()
            .filter_map(|installation| {
                state
                    .results
                    .get(&installation.id)
                    .map(|(status, _, _)| (installation, status))
            })
            .filter(
                |(installation, _)| match (&installation.announce_policy, state.task) {
                    (AnnouncePolicy::UpgradeOnly, Some(Task::Upgrade)) => true,
                    (AnnouncePolicy::AllTasks, _) => true,
                    (_, _) => false,
                },
            );

        let num_data = data.clone().count();

        if num_data > 0 {
            let mut blocks: Vec<SlackBlock> = Vec::with_capacity(num_data);

            let title = slack_title(state);
            let markdown = SlackBlockPlainTextOnly::from(title.clone());
            let heading = SlackHeaderBlock::new(markdown.into());
            blocks.push(heading.into());

            for (installation, status) in data {
                let version = installation.get_display_version();

                let verb = match state.task {
                    Some(Task::Upgrade) => "upgraded to",
                    Some(Task::Diff) => "diffed with",
                    Some(Task::Test) => "tested with",
                    Some(Task::Template) => "templated with",
                    Some(Task::Outdated) => "version checked with",
                    None => "processed",
                };
                let cluster = installation.cluster_name.clone();
                let string = format!(
                    "{status} {} was {verb} {version} on {cluster} namespace {}",
                    installation.name, installation.namespace
                );
                let markdown = SlackBlockMarkDownText::new(string);
                let block = SlackSectionBlock::new().with_text(markdown.into());
                blocks.push(block.into());
            }

            let content = SlackMessageContent::new()
                .with_blocks(blocks)
                .with_text(title);

            let client = SlackClient::new(SlackClientHyperConnector::new());
            let session = client.open_session(&self.token);
            let post_chat_req = SlackApiChatPostMessageRequest::new(
                self.announce_slack_channel.clone().into(),
                content,
            );
            session.chat_post_message(&post_chat_req).await?;
        }

        Ok(())
    }
}

fn get_installation_blocks(state: &State, title: &str) -> Vec<SlackBlock> {
    let status = results_to_string(state);
    let status = ["```".to_string(), status, "```".to_string()];
    let status = status.join("\n");
    let markdown = SlackBlockPlainTextOnly::from(title);
    let heading = SlackHeaderBlock::new(markdown.into());
    let markdown = SlackBlockMarkDownText::new(status);
    let block = SlackSectionBlock::new().with_text(markdown.into());
    let blocks = slack_blocks![some_into(heading), some_into(block)];
    blocks
}

fn get_outdated_blocks(state: &State) -> Vec<SlackBlock> {
    if state.versions.is_empty() {
        vec![]
    } else {
        let title = "Out of date installations";
        let status = versions_to_string(state);
        let status = ["```".to_string(), status, "```".to_string()];
        let status = status.join("\n");
        let markdown = SlackBlockPlainTextOnly::from(title);
        let heading = SlackHeaderBlock::new(markdown.into());
        let markdown = SlackBlockMarkDownText::new(status);
        let block = SlackSectionBlock::new().with_text(markdown.into());
        let blocks = slack_blocks![some_into(heading), some_into(block)];
        blocks
    }
}

fn slack_title(state: &State) -> String {
    let task = match state.task {
        Some(Task::Diff) => "diff",
        Some(Task::Upgrade) => "upgrade",
        Some(Task::Test) => "test",
        Some(Task::Template) => "template",
        Some(Task::Outdated) => "outdated",
        None => "unknown",
    };
    format!("helmci - {task}")
}

async fn update_results(state: &State, slack_state: &mut Option<SlackState>, finished: bool) {
    if let Some(slack) = slack_state {
        let mut errors = false;

        if let Err(err) = slack.update_slack(state).await {
            error!("Slack error: {err}");
            errors = true;
        }

        if finished {
            if let Err(err) = slack.send_finished(state).await {
                error!("Slack error: {err}");
                errors = true;
            }
        }

        if errors {
            *slack_state = None;
        }
    };

    if finished {
        let results = results_to_string(state);
        print!("\n\n{results}");

        if !state.versions.is_empty() {
            let versions = versions_to_string(state);
            print!("\n\n{versions}");
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

fn process_message(msg: Message, state: &mut State) {
    match msg {
        Message::InstallationResult(hr) => {
            let HelmResult {
                command,
                result,
                installation,
            } = &hr;
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
        Message::Skippedjob(installation) => {
            state
                .results
                .insert(installation.id, (Status::Skipped, None, None));
            state.jobs.push(installation);
        }
        Message::InstallationVersion(installation, our_version, upstream_version) => {
            if our_version != upstream_version {
                state
                    .versions
                    .insert(installation.id, (our_version, upstream_version));
            }
        }
        Message::Newjob(installation) => {
            state
                .results
                .insert(installation.id, (Status::Pending, None, None));
            state.jobs.push(installation);
        }
        Message::StartedJob(installation, start_instant) => {
            state.results.insert(
                installation.id,
                (Status::InProgess, Some(start_instant), None),
            );
        }
        Message::FinishedJob(installation, result, duration) => {
            let status = match result {
                Ok(_) => Status::Complete,
                Err(_) => Status::Failed,
            };
            state
                .results
                .insert(installation.id, (status, None, Some(duration)));
        }
        Message::FinishedAll(rc, duration) => {
            let status = match rc {
                Ok(_) => Status::Complete,
                Err(_) => Status::Failed,
            };
            state.finished = Some((status, duration));
        }
        Message::Start(task, start_instant) => {
            state.task = Some(task);
            state.start_instant = Some(start_instant);
        }
    }
}

#[derive(Clone)]
struct State {
    task: Option<Task>,
    start_instant: Option<Instant>,
    results: HashMap<InstallationId, (Status, Option<Instant>, Option<Duration>)>,
    versions: HashMap<InstallationId, (String, String)>,
    jobs: Vec<Installation>,
    finished: Option<(Status, Duration)>,
}
pub fn start() -> TextOutput {
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
        let mut interval = interval(Duration::from_secs(2));
        let mut slack_state = SlackState::new().ok();
        loop {
            select! {
                _ = interval.tick() => {
                    update_results(&state, &mut slack_state, false).await;
                },

                msg = rx.recv() => {
                    if let Some(msg) = msg {
                        process_message(msg, &mut state);
                    } else {
                        // Note interval.tick() will go for ever, so this is the main exit point.
                        // Will happen when sender closes rx pipe.
                        break;
                    };
                },

                else => {
                    // This will not get called.
                    break;
                }
            }
        }
        update_results(&state, &mut slack_state, true).await;
    });

    TextOutput {
        tx: Some(tx),
        thread: Some(thread),
    }
}

#[async_trait]
impl Output for TextOutput {
    fn get_tx(&mut self) -> Option<Sender> {
        self.tx.take()
    }

    async fn wait(&mut self) -> Result<()> {
        if let Some(tx) = self.tx.take() {
            drop(tx);
        }
        if let Some(thread) = self.thread.take() {
            thread.await?;
        }
        Ok(())
    }
}
