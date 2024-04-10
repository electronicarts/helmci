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
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tap::Pipe;
use tracing::{debug, error};
use url::Url;

use crate::{
    command::{CommandLine, CommandResult, CommandSuccess},
    config::{AnnouncePolicy, ReleaseReference},
    output::{Message, MultiOutput},
};

/// A reference to a Helm Repo.
#[derive(Debug)]
pub struct HelmRepo {
    pub name: String,
    pub url: String,
}

/// A reference to a helm chart.
#[derive(Debug)]
pub enum HelmChart {
    Dir(PathBuf),
    HelmRepo {
        repo: HelmRepo,
        chart_name: String,
        chart_version: String,
    },
    OciRepo {
        repo_url: String,
        chart_name: String,
        chart_version: String,
    },
}

fn helm_path() -> OsString {
    std::env::var_os("HELM_PATH").unwrap_or_else(|| "helm".into())
}

fn aws_path() -> OsString {
    std::env::var_os("AWS_PATH").unwrap_or_else(|| "aws".into())
}

/// A unique identifier for an installation.
pub type InstallationId = u16;

#[derive(Debug)]
pub enum ValuesFormat {
    PlainText,
    Vals,
    Sops,
}

#[derive(Debug)]
pub struct ValuesFile {
    pub path: PathBuf,
    pub format: ValuesFormat,
}

/// All the information required for an helm release to be processed.
#[derive(Debug)]
pub struct Installation {
    pub name: String,
    pub namespace: String,
    pub env_name: String,
    pub cluster_name: String,
    pub context: String,
    pub values_files: Vec<ValuesFile>,
    pub chart: HelmChart,
    pub depends: Vec<ReleaseReference>,
    pub timeout: u16,
    pub id: InstallationId,
    pub announce_policy: AnnouncePolicy,
}

impl Installation {
    pub fn get_display_version(&self) -> &str {
        match &self.chart {
            HelmChart::Dir(_) => "local",
            HelmChart::HelmRepo { chart_version, .. }
            | HelmChart::OciRepo { chart_version, .. } => chart_version,
        }
    }
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

    pub fn stdout(&self) -> &str {
        match &self.result {
            Ok(success) => &success.stdout,
            Err(err) => &err.stdout,
        }
    }

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
}

/// Request to add a repo to helm.
pub async fn add_repo(
    HelmRepo {
        name: repo_name,
        url: repo_url,
    }: &HelmRepo,
) -> Result<()> {
    debug!("Add helm repo {} at {}... ", repo_name, repo_url);

    let command: CommandLine = CommandLine(
        "helm".into(),
        vec![
            "repo".into(),
            "add".into(),
            "--force-update".into(),
            repo_name.into(),
            repo_url.into(),
        ],
    );

    let result = command.run().await;
    if let Err(err) = result {
        error!("error installing helm repo");
        error!("{}", err);
        anyhow::bail!("helm repo add failed");
    }

    debug!("done adding helm repo.");
    Ok(())
}

/// Request to remote a repo from helm.
pub async fn remove_repo(
    HelmRepo {
        name: repo_name,
        url: repo_url,
    }: &HelmRepo,
) -> Result<()> {
    debug!("Remove helm repo {} at {}... ", repo_name, repo_url);

    let command: CommandLine = CommandLine(
        "helm".into(),
        vec!["repo".into(), "remove".into(), repo_name.into()],
    );

    let result = command.run().await;
    if let Err(err) = result {
        error!("error removing helm repo");
        error!("{}", err);
        anyhow::bail!("helm repo remove failed");
    }

    debug!("done removing helm repo.");
    Ok(())
}

/// Run the lint command.
pub async fn lint(installation: &Arc<Installation>, tx: &MultiOutput) -> Result<()> {
    if let HelmChart::Dir(dir) = &installation.chart {
        let mut args = vec![
            "lint".into(),
            "--namespace".into(),
            installation.namespace.clone().into(),
            "--kube-context".into(),
            installation.context.clone().into(),
        ];

        args.append(&mut add_values_files(installation));
        args.push(dir.clone().into_os_string());
        args.append(&mut get_template_parameters(installation));

        let command_line = CommandLine(helm_path(), args);
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
fn get_args_from_chart(chart: &HelmChart) -> (OsString, Vec<OsString>) {
    let (chart_name, version) = match &chart {
        HelmChart::Dir(dir) => (dir.clone().into_os_string(), None),
        HelmChart::HelmRepo {
            repo,
            chart_name,
            chart_version,
        } => (
            format!("{}/{chart_name}", repo.name).into(),
            Some(chart_version),
        ),
        HelmChart::OciRepo {
            repo_url,
            chart_name,
            chart_version,
        } => (
            format!("{repo_url}/{chart_name}").into(),
            Some(chart_version),
        ),
    };

    let mut args = vec![];
    if let Some(version) = version {
        args.push("--version".into());
        args.push(version.into());
    }

    (chart_name, args)
}

/// Run the helm template command.
pub async fn template(installation: &Arc<Installation>, tx: &MultiOutput) -> Result<()> {
    let (chart, mut chart_args) = get_args_from_chart(&installation.chart);

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

    let command_line = CommandLine(helm_path(), args);
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

/// Run the helm diff command.
pub async fn diff(installation: &Arc<Installation>, tx: &MultiOutput) -> Result<()> {
    let (chart, mut chart_args) = get_args_from_chart(&installation.chart);

    let mut args = vec![
        "diff".into(),
        "upgrade".into(),
        installation.name.clone().into(),
        chart,
        "--context=3".into(),
        "--no-color".into(),
        "--allow-unreleased".into(),
        "--namespace".into(),
        installation.namespace.clone().into(),
        "--kube-context".into(),
        installation.clone().context.clone().into(),
    ];

    args.append(&mut add_values_files(installation));
    args.append(&mut chart_args);
    args.append(&mut get_template_parameters(installation));

    let command_line = CommandLine(helm_path(), args);
    let result = command_line.run().await;
    let has_errors = result.is_err();
    let i_result = HelmResult::from_result(installation, result, Command::Diff);
    let i_result = Arc::new(i_result);
    tx.send(Message::InstallationResult(i_result)).await;

    if has_errors {
        Err(anyhow::anyhow!("diff operation failed"))
    } else {
        Ok(())
    }
}

/// Run the helm upgrade command.
pub async fn upgrade(
    installation: &Arc<Installation>,
    tx: &MultiOutput,
    dry_run: bool,
) -> Result<()> {
    let (chart, mut chart_args) = get_args_from_chart(&installation.chart);

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

    let command_line = CommandLine(helm_path(), args);
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
pub async fn outdated(installation: &Arc<Installation>, tx: &MultiOutput) -> Result<()> {
    match &installation.chart {
        HelmChart::Dir(_) => {}
        HelmChart::HelmRepo {
            repo,
            chart_name,
            chart_version,
        } => outdated_helm_chart(installation, repo, chart_name, chart_version, tx).await?,
        HelmChart::OciRepo {
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
    repo: &HelmRepo,
    chart_name: &str,
    chart_version: &str,
    tx: &MultiOutput,
) -> Result<()> {
    let chart_version = parse_version(chart_version)
        .map_err(|err| anyhow::anyhow!("Failed to parse version {chart_version:?} {err:?}"))?;

    let args = vec![
        "search".into(),
        "repo".into(),
        "-o=json".into(),
        format!("{}/{chart_name}", repo.name).into(),
    ];

    let command_line = CommandLine(helm_path(), args);
    let result = command_line.run().await;
    let has_errors = result.is_err();

    if let Ok(CommandSuccess { stdout, .. }) = &result {
        let version: Vec<HelmVersionInfo> = serde_json::from_str(stdout)?;
        let version = version.first().ok_or_else(|| {
            anyhow::anyhow!("No version information found for chart {chart_name}")
        })?;
        let version = parse_version(&version.version)
            .map_err(|err| anyhow::anyhow!("Failed to parse version {version:?} {err:?}"))?;

        tx.send(Message::InstallationVersion(
            installation.clone(),
            chart_version,
            version,
        ))
        .await;
    };

    let i_result = HelmResult::from_result(installation, result, Command::Outdated);
    let i_result = Arc::new(i_result);
    tx.send(Message::InstallationResult(i_result)).await;

    if has_errors {
        Err(anyhow::anyhow!("outdated operation failed"))
    } else {
        Ok(())
    }
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
    name: String,
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
    ) -> Result<Version> {
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

                let command_line = CommandLine(aws_path(), args);
                let result = command_line.run().await;

                let rc = match &result {
                    Ok(CommandSuccess { stdout, .. }) => {
                        let details: OciDetails = serde_json::from_str(stdout)?;
                        get_latest_version_from_details(details)
                            .map_or_else(|| Err(anyhow::anyhow!("no versions found")), Ok)
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
                    .get(&format!("https://public.ecr.aws/v2/{path}/tags/list"))
                    .header("Authorization", format!("Bearer {}", token.token))
                    .send()
                    .await?
                    .json()
                    .await?;

                get_latest_version_from_tags(tags)
                    .map_or_else(|| Err(anyhow::anyhow!("no versions found")), Ok)
            }
        }
    }
}

/// Generate the outdated report for an OCI chart reference stored on ECR.
async fn outdated_oci_chart(
    installation: &Arc<Installation>,
    repo_url: &str,
    chart_name: &str,
    chart_version: &str,
    tx: &MultiOutput,
) -> Result<()> {
    if chart_version.starts_with("0.0.0+") {
        // Hack to skip charts that have git hash versions.
        // We can't do anything with these.
        return Ok(());
    }

    let chart_version = parse_version(chart_version)
        .map_err(|err| anyhow::anyhow!("Failed to parse version {chart_version:?} {err:?}"))?;

    let url = Url::parse(repo_url)?;
    let parsed = ParsedOci::new(&url, chart_name)?;

    let latest_version = parsed
        .get_latest_version(installation, tx)
        .await
        .map_err(|err| anyhow::anyhow!("Get latest version failed {err:?}"))?;
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
    let version = if tag.contains('.') {
        Version::parse(tag)?
    } else {
        let tag = &format!("{tag}.0.0");
        Version::parse(tag)?
    };
    Ok(version)
}

/// Get the latest version for the given `OciDetails`.
fn get_latest_version_from_details(details: OciDetails) -> Option<Version> {
    let mut versions = vec![];
    for image in details.image_details {
        if let Some(tags) = image.image_tags {
            for tag in tags {
                match parse_version(&tag) {
                    Ok(version) => versions.push(version),
                    Err(err) => error!("Cannot parse version {tag} {err}"),
                }
            }
        }
    }

    versions.sort();
    versions.last().cloned()
}

/// Get the latest version for the given `AwsTags`.
fn get_latest_version_from_tags(tags: AwsTags) -> Option<Version> {
    let mut versions = vec![];
    for tag in tags.tags {
        match parse_version(&tag) {
            Ok(version) => versions.push(version),
            Err(err) => error!("Cannot parse version {tag} {err}"),
        }
    }

    versions.sort();
    versions.last().cloned()
}
