// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.
use super::Message;
use super::Output;
use super::Sender;
use crate::config::AnnouncePolicy;
use crate::duration::duration_string;
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
use tokio::select;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tokio::time::Instant;

pub struct SlackOutput {
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

    async fn update_slack(&mut self, state: &State) -> Result<()> {
        if state.task.is_none() {
            return Ok(());
        }

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
            let update_response = session.chat_update(&update_req).await?;
            self.ts = Some(update_response.ts);
        } else {
            let post_chat_req =
                SlackApiChatPostMessageRequest::new(self.slack_channel.clone().into(), content);
            let post_chat_response = session.chat_post_message(&post_chat_req).await?;
            self.ts = Some(post_chat_response.ts);
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
            let mut blocks: Vec<SlackBlock> = Vec::with_capacity(num_data + 1);

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

    async fn log_helm_result(&self, state: &State, hr: &crate::helm::HelmResult) -> Result<()> {
        let mut blocks: Vec<SlackBlock> = Vec::with_capacity(5);

        let title = slack_title(state);
        let markdown = SlackBlockPlainTextOnly::from(title.clone());
        let heading = SlackHeaderBlock::new(markdown.into());
        blocks.push(heading.into());

        let cmd = match &hr.result {
            Ok(success) => &success.cmd,
            Err(err) => &err.cmd,
        };

        let markdown = SlackBlockPlainTextOnly::from(cmd.to_string());
        let block = SlackSectionBlock::new().with_text(markdown.into());
        blocks.push(block.into());

        let markdown = SlackBlockPlainTextOnly::from(hr.result_line());
        let block = SlackSectionBlock::new().with_text(markdown.into());
        blocks.push(block.into());

        let string = truncate(hr.stdout(), 150);
        if !string.is_empty() {
            let string = format!("```{string}```\n");
            let markdown = SlackBlockMarkDownText::from(string);
            let block = SlackSectionBlock::new().with_text(markdown.into());
            blocks.push(block.into());
        }

        let string = truncate(hr.stderr(), 150);
        if !string.is_empty() {
            let string = format!("```{string}```\n");
            let markdown = SlackBlockMarkDownText::from(string);
            let block = SlackSectionBlock::new().with_text(markdown.into());
            blocks.push(block.into());
        }

        let content = SlackMessageContent::new()
            .with_blocks(blocks)
            .with_text(title);

        let client = SlackClient::new(SlackClientHyperConnector::new());
        let session = client.open_session(&self.token);
        let post_chat_req =
            SlackApiChatPostMessageRequest::new(self.slack_channel.clone().into(), content);
        session.chat_post_message(&post_chat_req).await?;
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
        None => "unknown2",
    };
    format!("helmci - {task}")
}

async fn update_results(state: &State, slack: &mut SlackState, finished: bool) {
    if let Err(err) = slack.update_slack(state).await {
        println!("Slack error: {err}");
    }

    if finished {
        if let Err(err) = slack.send_finished(state).await {
            println!("Slack error: {err}");
        }
    }
}

async fn process_message(msg: &Arc<Message>, state: &mut State, slack: &SlackState) {
    match msg.as_ref() {
        Message::InstallationResult(hr) => {
            if hr.is_err() {
                slack
                    .log_helm_result(state, hr)
                    .await
                    .unwrap_or_else(|err| println!("Slack error: {err}"));
            }
        }
        Message::Log(_entry) => {}
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
pub fn start() -> Result<(SlackOutput, Sender)> {
    let (tx, mut rx) = mpsc::channel(50);
    let mut slack_state = SlackState::new()?;

    let thread = tokio::spawn(async move {
        let mut state = State {
            task: None,
            start_instant: None,
            results: HashMap::new(),
            versions: HashMap::new(),
            jobs: Vec::new(),
            finished: None,
        };

        // Slack will rate limit us if we send too many messages, so we need to throttle.
        let mut interval = interval(Duration::from_secs(5));
        loop {
            select! {
                _ = interval.tick() => {
                    update_results(&state, &mut slack_state, false).await;
                },

                msg = rx.recv() => {
                    if let Some(msg) = msg {
                        process_message(&msg, &mut state, &slack_state).await;
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

    Ok((
        SlackOutput {
            thread: Some(thread),
        },
        tx,
    ))
}

#[async_trait]
impl Output for SlackOutput {
    async fn wait(&mut self) -> Result<()> {
        if let Some(thread) = self.thread.take() {
            thread.await?;
        }
        Ok(())
    }
}
