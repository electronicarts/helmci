// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.

//! Define helm commands.
//!
//! This defines a list of commands that take a message to the output server and an installation to act on.
use anyhow::Result;
use semver::Version;
use serde::Deserialize;
use std::{
    ffi::OsString,
    fmt::{Debug, Display},
    fs::read_to_string,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tap::Pipe;
use url::Url;

use crate::{
    Update,
    command::{CommandLine, CommandResult, CommandSuccess},
    config::{AnnouncePolicy, ChartReference, ReleaseReference, ValuesFile, ValuesFormat},
    output::{Message, MultiOutput, debug, error, info},
    repos::{charts::Chart, helm},
};

fn helm_path() -> OsString {
    std::env::var_os("HELM_PATH").unwrap_or_else(|| "helm".into())
}

fn aws_path() -> OsString {
    std::env::var_os("AWS_PATH").unwrap_or_else(|| "aws".into())
}

/// A unique identifier for an installation.
pub type InstallationId = u16;

/// All the information required for an helm release to be processed.
#[derive(Debug)]
pub struct Installation {
    pub name: String,
    pub namespace: String,
    pub env_name: String,
    pub cluster_name: String,
    pub context: String,
    pub config_file: PathBuf,
    pub lock_file: PathBuf,
    pub values_files: Vec<ValuesFile>,
    pub chart_reference: ChartReference,
    pub depends: Vec<ReleaseReference>,
    pub timeout: u16,
    pub id: InstallationId,
    pub announce_policy: AnnouncePolicy,
}

impl Installation {
    pub fn get_display_version(&self) -> &str {
        match &self.chart_reference {
            ChartReference::Helm { chart_version, .. }
            | ChartReference::Oci { chart_version, .. } => chart_version.as_str(),
            ChartReference::Local { .. } => "local",
        }
    }
}

#[derive(Debug)]
pub struct DownloadedInstallation {
    pub installation: Arc<Installation>,
    pub chart: Option<Chart>,
}

/// What command was requested?
#[derive(Copy, Clone, Debug)]
pub enum Command {
    Lint,
    Diff,
    Template,
    UpgradeDry,
    Upgrade,
    Outdated,
}

impl Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        let str = match self {
            Command::Lint => "lint",
            Command::Diff => "diff",
            Command::Template => "template",
            Command::UpgradeDry => "upgrade(dry)",
            Command::Upgrade => "upgrade(real)",
            Command::Outdated => "outdated",
        };
        f.write_str(str)
    }
}

/// The results of running an installation command.
#[derive(Debug)]
pub struct HelmResult {
    pub installation: Arc<Installation>,
    pub result: CommandResult,
    pub command: Command,
}

impl HelmResult {
    fn from_result(
        installation: &Arc<Installation>,
        result: CommandResult,
        command: Command,
    ) -> Self {
        HelmResult {
            installation: installation.clone(),
            result,
            command,
        }
    }

    pub const fn is_err(&self) -> bool {
        self.result.is_err()
    }

    pub const fn duration(&self) -> Duration {
        match &self.result {
            Ok(success) => success.duration,
            Err(err) => err.duration,
        }
    }

    pub const fn command_line(&self) -> &CommandLine {
        match &self.result {
            Ok(success) => &success.cmd,
            Err(err) => &err.cmd,
        }
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn stdout(&self) -> &str {
        match &self.result {
            Ok(success) => &success.stdout,
            Err(err) => &err.stdout,
        }
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn stderr(&self) -> &str {
        match &self.result {
            Ok(success) => &success.stderr,
            Err(err) => &err.stderr,
        }
    }

    pub fn result_line(&self) -> String {
        match &self.result {
            Ok(success) => success.result_line(),
            Err(err) => err.result_line(),
        }
    }

    pub const fn exit_code(&self) -> i32 {
        match &self.result {
            Ok(success) => success.exit_code,
            Err(err) => err.exit_code,
        }
    }
}

/// Run the lint command.
pub async fn lint(downloaded: &DownloadedInstallation, tx: &MultiOutput) -> Result<()> {
    let installation = &downloaded.installation;
    if let ChartReference::Local { path } = &installation.chart_reference {
        let mut args = vec![
            "lint".into(),
            "--namespace".into(),
            installation.namespace.clone().into(),
            "--kube-context".into(),
            installation.context.clone().into(),
        ];

        args.append(&mut add_values_files(installation));
        args.push(path.clone().into_os_string());
        args.append(&mut get_template_parameters(installation));

        let command_line = CommandLine::new(helm_path(), args);
        let result = command_line.run().await;
        let has_errors = result.is_err();
        let i_result = HelmResult::from_result(installation, result, Command::Lint);
        let i_result = Arc::new(i_result);
        tx.send(Message::InstallationResult(i_result)).await;

        if has_errors {
            Err(anyhow::anyhow!("lint operation failed"))
        } else {
            Ok(())
        }
    } else {
        Ok(())
    }
}

fn add_values_files(installation: &Arc<Installation>) -> Vec<OsString> {
    let mut args = Vec::new();

    for values in &installation.values_files {
        args.push("-f".into());

        let file = match values.format {
            ValuesFormat::PlainText => values.path.clone().into_os_string(),
            ValuesFormat::Vals => {
                let mut str: OsString = "secrets://vals!".into();
                str.push(values.path.as_os_str());
                str
            }
            ValuesFormat::Sops => {
                let mut str: OsString = "secrets://sops!".into();
                str.push(values.path.as_os_str());
                str
            }
        };

        args.push(file);
    }

    args
}

/// Get the template parameters for a chart.
fn get_template_parameters(installation: &Installation) -> Vec<OsString> {
    vec![
        format!("--set=global.namespace={}", installation.namespace).into(),
        format!("--set=global.context={}", installation.context).into(),
        format!("--set=global.env_name={}", installation.env_name).into(),
        format!("--set=global.cluster_name={}", installation.cluster_name).into(),
    ]
}

/// Get the required helm arguments for this chart.
fn get_args_from_chart(chart: &Chart) -> (OsString, Vec<OsString>) {
    (chart.file_path.clone().into(), vec![])
}

/// Run the helm template command.
pub async fn template(downloaded: &DownloadedInstallation, tx: &MultiOutput) -> Result<()> {
    let installation = &downloaded.installation;
    let Some(chart) = &downloaded.chart else {
        return Err(anyhow::anyhow!("chart not downloaded"));
    };
    let (chart, mut chart_args) = get_args_from_chart(chart);

    let mut args = vec![
        "template".into(),
        installation.name.clone().into(),
        chart,
        "--is-upgrade".into(),
        "--namespace".into(),
        installation.namespace.clone().into(),
        "--kube-context".into(),
        installation.context.clone().into(),
    ];

    args.append(&mut add_values_files(installation));
    args.append(&mut chart_args);
    args.append(&mut get_template_parameters(installation));

    let command_line = CommandLine::new(helm_path(), args);
    let result = command_line.run().await;
    let has_errors = result.is_err();
    let i_result = HelmResult::from_result(installation, result, Command::Template);
    let i_result = Arc::new(i_result);
    tx.send(Message::InstallationResult(i_result)).await;

    if has_errors {
        Err(anyhow::anyhow!("template operation failed"))
    } else {
        Ok(())
    }
}

// The DiffResult struct is used to store the exit code of the diff command.
pub enum DiffResult {
    NoChanges,
    Changes,
}

/// Run the helm diff command.
pub async fn diff(downloaded: &DownloadedInstallation, tx: &MultiOutput) -> Result<DiffResult> {
    let installation = &downloaded.installation;
    let Some(chart) = &downloaded.chart else {
        return Err(anyhow::anyhow!("chart not downloaded"));
    };
    let (chart, mut chart_args) = get_args_from_chart(chart);

    // Construct the arguments for the helm diff command.
    let mut args = vec![
        "diff".into(),
        "upgrade".into(),
        installation.name.clone().into(),
        chart,
        "--detailed-exitcode".into(), // This flag ensures that the exit code will indicate if there are changes or errors.
        "--context=3".into(),
        "--no-color".into(),
        "--allow-unreleased".into(),
        "--namespace".into(),
        installation.namespace.clone().into(),
        "--kube-context".into(),
        installation.clone().context.clone().into(),
        "--show-secrets".into(),
    ];

    // Append additional arguments from the installation configuration.
    args.append(&mut add_values_files(installation));
    args.append(&mut chart_args);
    args.append(&mut get_template_parameters(installation));

    // Create a CommandLine instance with the helm path and the constructed arguments.
    let command_line = CommandLine::new(helm_path(), args).with_allowed_exit_codes(vec![0, 2]);
    // Run the command and await the result.
    let result = command_line.run().await;
    // Create a HelmResult instance from the command result.
    let i_result = HelmResult::from_result(installation, result, Command::Diff);

    // Evaluate the detailed exit code - any non-zero exit code indicates changes (1) or errors (2).
    let diff_result = match &i_result.result {
        Ok(CommandSuccess { exit_code: 0, .. }) => {
            debug!(tx, "No changes detected!").await; // Exit code 0 indicates no changes.
            Ok(DiffResult::NoChanges)
        }
        Ok(CommandSuccess { exit_code: 1, .. }) => {
            error!(tx, "errors encountered!").await; // exit code 1 indicates errors.
            Err(anyhow::anyhow!("diff operation failed"))
        }
        Ok(CommandSuccess { exit_code: 2, .. }) => {
            debug!(tx, "Changes detected!").await; // Exit code 2 indicates changes.
            Ok(DiffResult::Changes)
        }
        Ok(CommandSuccess { .. }) => {
            error!(tx, "Unknown exit code").await; // Any other exit code is considered unknown.
            Err(anyhow::anyhow!(
                "diff operation returned unknown error code"
            ))
        }
        Err(err) => {
            debug!(tx, "Other exception encountered").await; // If the command result is an error, return Unknown.
            Err(anyhow::anyhow!(
                "diff operation failed: {}",
                err.to_string()
            ))
        }
    };

    // Wrap the HelmResult in an Arc and send it via the MultiOutput channel.
    let i_result = Arc::new(i_result);
    tx.send(Message::InstallationResult(i_result.clone())).await;

    // Return the diff result.
    diff_result
}

/// Run the helm upgrade command.
pub async fn upgrade(
    downloaded: &DownloadedInstallation,
    tx: &MultiOutput,
    dry_run: bool,
) -> Result<()> {
    let installation = &downloaded.installation;
    let Some(chart) = &downloaded.chart else {
        return Err(anyhow::anyhow!("chart not downloaded"));
    };
    let (chart, mut chart_args) = get_args_from_chart(chart);

    let mut args = vec![
        "upgrade".into(),
        installation.name.clone().into(),
        chart,
        "--install".into(),
        "--wait".into(),
        "--timeout".into(),
        format!("{}s", installation.timeout).into(),
        "--namespace".into(),
        installation.namespace.clone().into(),
        "--kube-context".into(),
        installation.context.clone().into(),
    ];

    args.append(&mut add_values_files(installation));
    args.append(&mut chart_args);
    args.append(&mut get_template_parameters(installation));

    if dry_run {
        args.push("--dry-run".into());
    }

    let command_line = CommandLine::new(helm_path(), args);
    let result = command_line.run().await;
    let has_errors = result.is_err();
    let command = if dry_run {
        Command::UpgradeDry
    } else {
        Command::Upgrade
    };
    let i_result = HelmResult::from_result(installation, result, command);
    let i_result = Arc::new(i_result);
    tx.send(Message::InstallationResult(i_result)).await;

    if has_errors {
        Err(anyhow::anyhow!("upgrade operation failed"))
    } else {
        Ok(())
    }
}

/// Run the helm outdated command.
pub async fn outdated(downloaded: &DownloadedInstallation, tx: &MultiOutput) -> Result<()> {
    let installation = &downloaded.installation;

    match &installation.chart_reference {
        ChartReference::Local { .. } => {}
        ChartReference::Helm {
            repo_url,
            chart_name,
            chart_version,
        } => outdated_helm_chart(installation, repo_url, chart_name, chart_version, tx).await?,
        ChartReference::Oci {
            repo_url,
            chart_name,
            chart_version,
        } => outdated_oci_chart(installation, repo_url, chart_name, chart_version, tx).await?,
    }
    Ok(())
}

/// Parser to interpret version information from helm.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct HelmVersionInfo {
    app_version: String,
    description: String,
    name: String,
    version: String,
}

/// Generate the outdated report for a helm chart reference.
async fn outdated_helm_chart(
    installation: &Arc<Installation>,
    repo_url: &Url,
    chart_name: &str,
    chart_version: &str,
    tx: &MultiOutput,
) -> Result<()> {
    if is_ignorable_tag(chart_version) {
        return Ok(());
    }

    let chart_version = parse_version(chart_version)
        .map_err(|err| anyhow::anyhow!("Failed to parse version {chart_version:?} {err:?}"))?;

    let repo = helm::Repo::download_index(repo_url).await?;

    match repo.get_newest_version(chart_name) {
        Ok(version) => {
            tx.send(Message::InstallationVersion(
                installation.clone(),
                chart_version,
                version,
            ))
            .await;
        }
        Err(err) => {
            return Err(err.into());
        }
    }

    Ok(())
}

/// Parser to interpret OCI information.
#[derive(Deserialize, Debug)]
struct OciDetails {
    #[serde(rename = "imageDetails")]
    image_details: Vec<ImageDetails>,
}

/// Parser to interpret image details from ECR.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct ImageDetails {
    registry_id: String,
    repository_name: String,
    image_digest: String,
    image_tags: Option<Vec<String>>,
    image_size_in_bytes: u32,
    // This can be a float value or a string "2023-11-09T14:07:41+11:00"
    // Probably depends on aws version.
    // image_pushed_at: f32,
    image_manifest_media_type: String,
    artifact_media_type: String,
}

/// Public AWS token
#[derive(Deserialize, Debug)]
struct AwsToken {
    token: String,
}

/// Parsed Public OCI tags
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct AwsTags {
    name: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

enum ParsedOci {
    Private {
        account: String,
        region: String,
        path: String,
    },
    Public {
        path: String,
    },
}

impl ParsedOci {
    fn new(url: &Url, chart_name: &str) -> Result<Self> {
        let host = url
            .host()
            .ok_or_else(|| anyhow::anyhow!("invalid repo url"))?;
        let host_split = match host {
            url::Host::Domain(host) => host.split('.').collect::<Vec<_>>(),
            _ => return Err(anyhow::anyhow!("invalid repo url, expected hostname")),
        };
        if host_split.len() == 6
            && host_split[1] == "dkr"
            && host_split[2] == "ecr"
            && host_split[4] == "amazonaws"
            && host_split[5] == "com"
        {
            let account = host_split[0].to_string();
            let region = host_split[3].to_string();
            let path = format!("{}/{chart_name}", url.path().trim_start_matches('/'));

            Self::Private {
                account,
                region,
                path,
            }
            .pipe(Ok)
        } else if host_split == ["public", "ecr", "aws"] {
            let path = format!("{chart_name}/{chart_name}");
            Self::Public { path }.pipe(Ok)
        } else {
            return Err(anyhow::anyhow!(
                "Unsupported OCI repo url {url}",
                url = url.to_string()
            ));
        }
    }

    async fn get_latest_version(
        &self,
        installation: &Arc<Installation>,
        tx: &MultiOutput,
    ) -> Result<Option<Version>> {
        match self {
            ParsedOci::Private {
                account,
                region,
                path,
            } => {
                let args: Vec<OsString> = vec![
                    "ecr".into(),
                    "describe-images".into(),
                    "--registry-id".into(),
                    account.into(),
                    "--region".into(),
                    region.into(),
                    "--repository-name".into(),
                    path.into(),
                ];

                let command_line = CommandLine::new(aws_path(), args);
                let result = command_line.run().await;

                let rc = match &result {
                    Ok(CommandSuccess { stdout, .. }) => {
                        let details: OciDetails = serde_json::from_str(stdout)?;
                        get_latest_version_from_details(details, tx)
                            .await
                            .map_or_else(
                                || Err(anyhow::anyhow!("no versions found")),
                                |v| Ok(Some(v)),
                            )
                    }
                    Err(err) => Err(anyhow::anyhow!("The describe-images command failed: {err}")),
                };

                let i_result = HelmResult::from_result(installation, result, Command::Outdated);
                let i_result = Arc::new(i_result);
                tx.send(Message::InstallationResult(i_result)).await;

                rc
            }
            ParsedOci::Public { path } => {
                let token: AwsToken = reqwest::get("https://public.ecr.aws/token/")
                    .await?
                    .json()
                    .await?;

                let tags: AwsTags = reqwest::Client::new()
                    .get(format!("https://public.ecr.aws/v2/{path}/tags/list"))
                    .header("Authorization", format!("Bearer {}", token.token))
                    .send()
                    .await?
                    .json()
                    .await?;

                get_latest_version_from_tags(path, tags, tx).await.pipe(Ok)
            }
        }
    }
}

/// Generate the outdated report for an OCI chart reference stored on ECR.
async fn outdated_oci_chart(
    installation: &Arc<Installation>,
    repo_url: &Url,
    chart_name: &str,
    chart_version: &str,
    tx: &MultiOutput,
) -> Result<()> {
    if is_ignorable_tag(chart_version) {
        return Ok(());
    }

    let chart_version = parse_version(chart_version)
        .map_err(|err| anyhow::anyhow!("Failed to parse version {chart_version:?} {err:?}"))?;

    let parsed = ParsedOci::new(repo_url, chart_name)?;

    let latest_version = parsed
        .get_latest_version(installation, tx)
        .await
        .map_err(|err| anyhow::anyhow!("Get latest version failed {err:?}"))?
        // if we can't get version information, just assume installed version is latest.
        .unwrap_or_else(|| chart_version.clone());
    tx.send(Message::InstallationVersion(
        installation.clone(),
        chart_version,
        latest_version,
    ))
    .await;

    Ok(())
}

/// Parse a semver complaint version.
fn parse_version(tag: &str) -> Result<Version> {
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(tag)?.pipe(Ok)
}

/// Get the latest version for the given `OciDetails`.
async fn get_latest_version_from_details(
    details: OciDetails,
    output: &MultiOutput,
) -> Option<Version> {
    let mut versions = vec![];
    for image in details.image_details {
        if let Some(tags) = image.image_tags {
            for tag in tags {
                if is_ignorable_tag(&tag) {
                    continue;
                }
                match parse_version(&tag) {
                    Ok(version) => versions.push(version),
                    Err(err) => {
                        error!(
                            output,
                            "Cannot parse version {} {tag}: {err}", image.repository_name
                        )
                        .await;
                    }
                }
            }
        }
    }

    versions.sort();
    versions.last().cloned()
}

/// Get the latest version for the given `AwsTags`.
async fn get_latest_version_from_tags(
    path: &str,
    tags: AwsTags,
    output: &MultiOutput,
) -> Option<Version> {
    let mut versions = vec![];
    for tag in tags.tags {
        if is_ignorable_tag(&tag) {
            continue;
        }
        match parse_version(&tag) {
            Ok(version) => versions.push(version),
            Err(err) => error!(output, "Cannot parse version {path} {tag}: {err}").await,
        }
    }

    versions.sort();
    versions.last().cloned()
}

// Check if version is non semver compliant or legacy and should be ignored
fn is_ignorable_tag(tag: &str) -> bool {
    tag.starts_with("v0-")
        || tag.starts_with("sha256-")
        || tag.starts_with("0.0.0_")
        || !tag.contains('.')
}

pub async fn update(
    installation: &Arc<DownloadedInstallation>,
    tx: &MultiOutput,
    updates: &Vec<Update>,
) -> Result<()> {
    let path = &installation.installation.config_file;
    let file = read_to_string(path)?;
    let mut doc = nondestructive::yaml::from_slice(file)?;

    let mut mapping = doc
        .as_mut()
        .into_mapping_mut()
        .ok_or_else(|| anyhow::anyhow!("Not a mapping"))?;

    let mut release_chart = mapping
        .get_mut("release_chart")
        .ok_or_else(|| anyhow::anyhow!("release_chart not found"))?;

    let mut release_chart = release_chart
        .as_mapping_mut()
        .ok_or_else(|| anyhow::anyhow!("release_chart is not a mapping"))?;

    for update in updates {
        let name = update.name.as_str();
        let value = update.value.as_str();
        if let Some(mut field) = release_chart.get_mut(name) {
            field.set_string(value);
        } else {
            return Err(anyhow::anyhow!("field {name} not found"));
        }
    }

    let file = doc.to_string();

    info!(tx, "Updating {path} to:\n{file}", path = path.display()).await;
    std::fs::write(path, file)?;
    Ok(())
}
