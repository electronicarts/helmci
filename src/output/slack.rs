// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.
use super::JobSuccess;
use super::Message;
use super::Output;
use super::Sender;
use crate::Request;
use crate::config::AnnouncePolicy;
use crate::duration::duration_string;
use crate::helm::Installation;
use crate::helm::InstallationId;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use futures::Future;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use serde_json::json;
use slack_morphism::errors::SlackClientError;
use slack_morphism::prelude::*;
use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;
use tabled::Table;
use tabled::Tabled;
use tabled::settings::Alignment;
use tabled::settings::Modify;
use tabled::settings::Style;
use tabled::settings::object::Rows;
use tokio;
use tokio::select;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio::time::sleep_until;

const DEFAULT_RETRY: Duration = Duration::from_secs(5);
const MAX_TRIES: u32 = 8;

pub struct SlackOutput {
    thread: Option<JoinHandle<()>>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
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
            Status::Skipped => '⏭',
            Status::Failed => '❌',
        };
        f.write_char(char)
    }
}

struct DisplayableDuration(Option<Duration>);

impl DisplayableDuration {
    const fn is_some(&self) -> bool {
        self.0.is_some()
    }
}

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

fn filter_stderr(s: &str) -> String {
    // Filter out useless messages from HELM, so good messages don't get truncated.
    s.lines()
        .filter(|line| !line.starts_with("Pulled: "))
        .filter(|line| !line.starts_with("Digest: "))
        .collect::<Vec<&str>>()
        .join("\n")
}

fn truncate(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        None => s,
        Some((idx, _)) => &s[..idx],
    }
}

fn results_to_vec(state: &State) -> Vec<JobResult> {
    state
        .jobs
        .iter()
        .filter_map(|installation| {
            state
                .results
                .get(&installation.id)
                .map(|result| to_job_result(result, installation))
        })
        .collect()
}

fn results_to_totals(state: &State) -> (Status, DisplayableDuration) {
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

    (status, DisplayableDuration(duration))
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
    client: SlackClient<SlackClientHyperConnector<HttpsConnector<HttpConnector>>>,
}

async fn retry_slack<Fut, F, R>(f: F) -> Option<R>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = Result<R, SlackClientError>> + Send,
    R: Send,
{
    let mut count = 0u32;
    loop {
        count += 1;
        match f().await {
            Err(SlackClientError::RateLimitError(rle)) => {
                let retry_time = Instant::now() + rle.retry_after.unwrap_or(DEFAULT_RETRY);
                sleep_until(retry_time).await;
                if count >= MAX_TRIES {
                    println!("Too many retries posting to slack: {rle}");
                    break None;
                }
            }
            Err(err) => {
                println!("Slack error posting: {err}");
                break None;
            }
            Ok(result) => break Some(result),
        }
    }
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

        let client = SlackClient::new(SlackClientHyperConnector::new()?);

        Ok(SlackState {
            token,
            slack_channel,
            announce_slack_channel,
            ts: None,
            client,
        })
    }

    async fn update_slack(&mut self, state: &State) -> Result<(), SlackClientError> {
        if state.request.is_none() {
            return Ok(());
        }

        let session = self.client.open_session(&self.token);
        let content = get_update_content(state);

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

    async fn update_slack_final(&mut self, state: &State) {
        let session = self.client.open_session(&self.token);
        let content = get_update_content(state);

        if let Some(ts) = &self.ts {
            let update_req = SlackApiChatUpdateRequest::new(
                self.slack_channel.clone().into(),
                content,
                ts.clone(),
            );
            let do_post = || async { session.chat_update(&update_req).await };
            if let Some(update_response) = retry_slack(do_post).await {
                self.ts = Some(update_response.ts);
            }
        } else {
            let post_chat_req =
                SlackApiChatPostMessageRequest::new(self.slack_channel.clone().into(), content);

            let do_post = || async { session.chat_post_message(&post_chat_req).await };
            if let Some(post_chat_response) = retry_slack(do_post).await {
                self.ts = Some(post_chat_response.ts);
            }
        }
    }

    async fn send_finished(&self, state: &State) {
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
            .filter(|(installation, _)| {
                match (&installation.announce_policy, state.request.as_deref()) {
                    (AnnouncePolicy::UpgradeOnly, Some(Request::Upgrade { .. })) => true,
                    (AnnouncePolicy::AllTasks, _) => true,
                    (_, _) => false,
                }
            });

        let num_data = data.clone().count();

        if num_data > 0 {
            let mut blocks: Vec<SlackBlock> = Vec::with_capacity(num_data + 1);

            let title = slack_title(state);
            let text = SlackBlockPlainTextOnly::from(title.clone());
            let heading = SlackHeaderBlock::new(text);
            blocks.push(heading.into());

            for (installation, status) in data {
                let version = installation.get_display_version();

                let verb = match state.request.as_deref() {
                    Some(Request::Upgrade { .. }) => "upgraded to",
                    Some(Request::Diff { .. }) => "diffed with",
                    Some(Request::Test { .. }) => "tested with",
                    Some(Request::Template { .. }) => "templated with",
                    Some(Request::Outdated { .. }) => "version checked with",
                    Some(Request::Update { .. }) => "updated values",
                    Some(Request::RewriteLocks) => "rewrote locks",
                    None => "processed",
                };
                let cluster = installation.cluster_name.clone();
                let string = format!(
                    "{status} {} was {verb} {version} on {cluster} namespace {}",
                    installation.name, installation.namespace
                );
                let text = SlackBlockPlainTextOnly::from(string);
                let block = SlackSectionBlock::new().with_text(text.into());
                blocks.push(block.into());
            }

            let content = SlackMessageContent::new()
                .with_blocks(blocks)
                .with_text(title);

            let session = self.client.open_session(&self.token);
            let post_chat_req = SlackApiChatPostMessageRequest::new(
                self.announce_slack_channel.clone().into(),
                content,
            );

            let do_post = || async { session.chat_post_message(&post_chat_req).await };
            retry_slack(do_post).await;
        }
    }

    async fn log_helm_result(&self, state: &State, hr: &crate::helm::HelmResult) {
        let mut blocks: Vec<SlackBlock> = Vec::with_capacity(5);

        let title = slack_title(state);
        let text = SlackBlockPlainTextOnly::from(title.clone());
        let heading = SlackHeaderBlock::new(text);
        blocks.push(heading.into());

        let cmd = match &hr.result {
            Ok(success) => &success.cmd,
            Err(err) => &err.cmd,
        };

        let text = SlackBlockPlainTextOnly::from(cmd.to_string());
        let block = SlackSectionBlock::new().with_text(text.into());
        blocks.push(block.into());

        let text = SlackBlockPlainTextOnly::from(hr.result_line());
        let block = SlackSectionBlock::new().with_text(text.into());
        blocks.push(block.into());

        let string = truncate(hr.stdout(), 150);
        if !string.is_empty() {
            let block = preformat_block(string);
            blocks.push(block);
        }

        let stderr = filter_stderr(hr.stderr());
        let string = truncate(&stderr, 150);
        if !string.is_empty() {
            let block = preformat_block(string);
            blocks.push(block);
        }

        let content = SlackMessageContent::new()
            .with_blocks(blocks)
            .with_text(title);

        let session = self.client.open_session(&self.token);
        let post_chat_req =
            SlackApiChatPostMessageRequest::new(self.slack_channel.clone().into(), content);

        let do_post = || async { session.chat_post_message(&post_chat_req).await };
        retry_slack(do_post).await;
    }
}

fn get_update_content(state: &State) -> SlackMessageContent {
    let title = slack_title(state);
    let mut installation_blocks = get_installation_blocks(state);
    let mut outdated_blocks = get_outdated_blocks(state);

    let mut blocks = vec![];
    blocks.append(&mut installation_blocks);
    blocks.append(&mut outdated_blocks);

    SlackMessageContent::new()
        .with_blocks(blocks)
        .with_text(title)
}

fn preformat_block(string: &str) -> SlackBlock {
    let value = json!({
        "elements": [{
            "type": "rich_text_preformatted",
            "elements": [{
                "type": "text",
                "text": string
            }]
        }]
    });
    SlackBlock::RichText(value)
}

fn get_installation_blocks(state: &State) -> Vec<SlackBlock> {
    let vec = results_to_vec(state);
    let totals = results_to_totals(state);

    let mut blocks: Vec<SlackBlock> = vec![];

    for status in [
        Status::Skipped,
        Status::Pending,
        Status::InProgress,
        Status::Complete,
        Status::Failed,
    ] {
        let status_vec: Vec<&JobResult> = vec.iter().filter(|x| x.status == status).collect();

        if !status_vec.is_empty() {
            {
                let title = slack_status_title(state, status);
                let text = SlackBlockPlainTextOnly::from(title);
                let heading = SlackHeaderBlock::new(text);
                blocks.push(heading.into());
            }

            {
                let elements: Vec<_> = status_vec
                    .iter()
                    .map(|x| {
                        let mut elements = vec![
                            json!({
                              "type": "text",
                              "text": format!("{}/{}/", x.cluster, x.namespace)
                            }),
                            json!({
                              "type": "text",
                              "text": x.release,
                              "style": {
                                  "bold": true
                              }
                            }),
                        ];

                        if x.duration.is_some() {
                            elements.push(json!({
                                "type": "text",
                                "text": format!(" ({})", x.duration),
                            }));
                        }

                        json!({
                            "type": "rich_text_section",
                            "elements": elements
                        })
                    })
                    .collect();

                let value = json!({
                    "elements": elements
                });
                let block = SlackBlock::RichText(value);
                blocks.push(block);
            }
        }
    }

    {
        let title = "Result";
        let text = SlackBlockPlainTextOnly::from(title);
        let heading = SlackHeaderBlock::new(text);
        blocks.push(heading.into());
    }

    {
        let value = json!({
            "elements": [
                {
                    "type": "rich_text_section",
                    "elements": [
                        {
                          "type": "text",
                          "text": format!("{} {}", totals.0, totals.1)
                        }
                    ]
                }]
        });
        let block = SlackBlock::RichText(value);
        blocks.push(block);
    }

    blocks
}

fn get_outdated_blocks(state: &State) -> Vec<SlackBlock> {
    if state.versions.is_empty() {
        vec![]
    } else {
        let title = "Out of date installations";
        let status = versions_to_string(state);
        let text = SlackBlockPlainTextOnly::from(title);
        let heading = SlackHeaderBlock::new(text);
        let block = preformat_block(&status);
        let blocks = slack_blocks![some_into(heading), some_into(block)];
        blocks
    }
}

fn slack_title(state: &State) -> String {
    let task = match state.request.as_deref() {
        Some(Request::Diff { .. }) => "diff",
        Some(Request::Upgrade { .. }) => "upgrade",
        Some(Request::Test { .. }) => "test",
        Some(Request::Template { .. }) => "template",
        Some(Request::Outdated { .. }) => "outdated",
        Some(Request::Update { .. }) => "update",
        Some(Request::RewriteLocks) => "rewrite locks",
        None => "unknown",
    };
    format!("helmci - {task}")
}

fn slack_status_title(state: &State, status: Status) -> String {
    let task = match state.request.as_deref() {
        Some(Request::Diff { .. }) => "diff",
        Some(Request::Upgrade { .. }) => "upgrade",
        Some(Request::Test { .. }) => "test",
        Some(Request::Template { .. }) => "template",
        Some(Request::Outdated { .. }) => "outdated",
        Some(Request::Update { .. }) => "update",
        Some(Request::RewriteLocks) => "rewrite locks",
        None => "unknown",
    };
    let status = match status {
        Status::Pending => "Pending",
        Status::InProgress => "In Progress",
        Status::Complete => "Complete",
        Status::Skipped => "Skipped",
        Status::Failed => "Failed",
    };
    format!("helmci - {task} - {status}")
}

async fn update_results(state: &State, slack: &mut SlackState) -> Instant {
    let update_time = match slack.update_slack(state).await {
        Err(SlackClientError::RateLimitError(err)) => err.retry_after.unwrap_or(DEFAULT_RETRY),
        Err(err) => {
            println!("Slack error updating result: {err}");
            DEFAULT_RETRY
        }
        Ok(()) => Duration::from_secs(1),
    };

    Instant::now() + update_time
}

async fn update_final_results(state: &State, slack: &mut SlackState) {
    slack.update_slack_final(state).await;
    slack.send_finished(state).await;
}

async fn process_message(msg: &Arc<Message>, state: &mut State, slack: &SlackState) {
    match msg.as_ref() {
        Message::InstallationResult(hr) => {
            if hr.is_err() {
                slack.log_helm_result(state, hr).await;
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
                    (our_version.to_string(), upstream_version.to_string()),
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
                Ok(JobSuccess::Completed) => Status::Complete,
                Ok(JobSuccess::Skipped) => Status::Skipped,
                Err(_) => Status::Failed,
            };
            state
                .results
                .insert(installation.id, (status, None, Some(*duration)));
        }
        Message::FinishedAll(rc, duration) => {
            let status = match rc {
                Ok(()) => Status::Complete,
                Err(_) => Status::Failed,
            };
            state.finished = Some((status, *duration));
        }
        Message::Start(request, start_instant) => {
            state.request = Some(request.clone());
            state.start_instant = Some(*start_instant);
        }
    }
}

#[derive(Clone)]
struct State {
    request: Option<Arc<Request>>,
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
            request: None,
            start_instant: None,
            results: HashMap::new(),
            versions: HashMap::new(),
            jobs: Vec::new(),
            finished: None,
        };

        // Slack will rate limit us if we send too many messages, so we need to throttle.
        let mut poll = Instant::now() + Duration::from_secs(1);
        loop {
            select! {
                () = sleep_until(poll) => {
                    poll = update_results(&state, &mut slack_state).await;
                },

                msg = rx.recv() => {
                    match msg { Some(msg) => {
                        process_message(&msg, &mut state, &slack_state).await;
                    } _ => {
                        // Note interval.tick() will go for ever, so this is the main exit point.
                        // Will happen when sender closes rx pipe.
                        break;
                    }}
                },

                else => {
                    // This will not get called.
                    break;
                }
            }
        }
        update_final_results(&state, &mut slack_state).await;
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
