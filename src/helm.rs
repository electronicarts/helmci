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
        tx.send(Message::InstallationVersion(
            installation.clone(),
            chart_version.to_owned(),
            version[0].version.clone(),
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
    image_pushed_at: f32,
    image_manifest_media_type: String,
    artifact_media_type: String,
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

    let url = Url::parse(repo_url)?;

    let args = vec![
        "ecr".into(),
        "describe-images".into(),
        "--region=us-east-2".into(),
        "--repository-name".into(),
        format!("{}/{chart_name}", url.path().trim_start_matches('/')).into(),
    ];

    let command_line = CommandLine(aws_path(), args);
    let result = command_line.run().await;
    let has_errors = result.is_err();

    if let Ok(CommandSuccess { stdout, .. }) = &result {
        let details: OciDetails = serde_json::from_str(stdout)?;
        if let Some(version) = get_latest_version(details) {
            tx.send(Message::InstallationVersion(
                installation.clone(),
                chart_version.to_owned(),
                version,
            ))
            .await;
        }
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

/// Parse a semver complaint version.
fn parse_version(tag: &str) -> Option<Version> {
    Version::parse(tag).ok()
}

/// Get the latest version for the given `OciDetails`.
fn get_latest_version(details: OciDetails) -> Option<String> {
    let mut versions = vec![];
    for image in details.image_details {
        if let Some(tags) = image.image_tags {
            for tag in tags {
                if let Some(version) = parse_version(&tag) {
                    versions.push(version);
                }
            }
        }
    }

    versions.sort();
    versions.last().map(std::string::ToString::to_string)
}
