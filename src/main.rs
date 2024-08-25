// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.

//! The main helmci program code
#![warn(missing_docs)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::use_self)]

extern crate lazy_static;

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::{self, FromStr};
use std::sync::Arc;

use anyhow::Result;
use anyhow::{anyhow, Context};

use clap::Parser;
use clap::Subcommand;

use futures::Future;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing_subscriber::util::SubscriberInitExt;

use tracing::{error, trace, warn, Level};
use tracing_subscriber::{Layer, Registry};

mod command;

mod helm;
use helm::{HelmChart, Installation};
use helm::{HelmRepo, InstallationId};

mod depends;
use depends::{is_depends_ok, HashIndex, InstallationSet};

mod output;
use output::{Message, MultiOutput, Output, Sender};

mod config;
use config::Release;
use config::{ChartReference, Cluster, Env};
use config::{Overrides, ValuesFile, ValuesFormat};

mod layer;
use layer::log;
use layer::CustomLayer;

mod duration;

mod utils;

/// An individual update
#[derive(Clone, Debug)]
pub struct Update {
    /// The name of the value to change
    pub name: String,
    /// The new value to change to
    pub value: String,
}

impl FromStr for Update {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.splitn(2, '=');
        let name = split
            .next()
            .ok_or_else(|| anyhow!("invalid update: {}", s))?
            .to_string();
        let value = split
            .next()
            .ok_or_else(|| anyhow!("invalid update: {}", s))?
            .to_string();
        Ok(Update { name, value })
    }
}

fn get_required_repos(todo: &[Arc<Installation>]) -> Vec<HelmRepo> {
    let mut repos = HashMap::with_capacity(todo.len());
    let mut next_id = 0;

    for installation in todo {
        if let ChartReference::Helm { repo_url, .. } = &installation.chart_reference {
            repos.insert(
                repo_url,
                HelmRepo {
                    name: format!("helm_{next_id}"),
                    url: repo_url.clone(),
                },
            );
            next_id += 1;
        }
    }

    repos.into_values().collect()
}

struct HelmReposLock(Vec<HelmRepo>, Option<oneshot::Sender<()>>);

impl HelmReposLock {
    fn get_helm_chart<'a>(&'a self, reference: &ChartReference) -> Result<HelmChart<'a>> {
        match reference {
            ChartReference::Helm {
                repo_url,
                chart_name,
                chart_version,
            } => {
                let chart_name = chart_name.clone();
                let chart_version = chart_version.clone();
                let repo = self
                    .0
                    .iter()
                    .find(|repo| repo.url == *repo_url)
                    .ok_or_else(|| anyhow!("no such repo: {}", repo_url))?;
                Ok(HelmChart::HelmRepo {
                    repo,
                    chart_name,
                    chart_version,
                })
            }
            ChartReference::Oci {
                repo_url,
                chart_name,
                chart_version,
            } => {
                let chart_name = chart_name.clone();
                let chart_version = chart_version.clone();
                let repo_url = repo_url.clone();
                Ok(HelmChart::OciRepo {
                    repo_url,
                    chart_name,
                    chart_version,
                })
            }
            ChartReference::Local { path } => Ok(HelmChart::Dir(path.clone())),
        }
    }
}

impl Drop for HelmReposLock {
    fn drop(&mut self) {
        // Send signal saying value has been dropped.
        if let Some(sender) = self.1.take() {
            if sender.send(()) == Err(()) {
                error!("failed to send drop signal");
            }
        }
    }
}

async fn with_helm_repos<T, FUT, FN>(repos: Vec<HelmRepo>, f: FN) -> Result<T>
where
    T: Sized + Send,
    FUT: Future<Output = T> + Send,
    FN: FnOnce(Arc<HelmReposLock>) -> FUT + Send,
{
    // Add these repos
    for repo in &repos {
        helm::add_repo(repo).await?;
    }

    let clone = repos.clone();
    let (tx, rx) = oneshot::channel();
    let lock = Arc::new(HelmReposLock(repos, Some(tx)));

    let rc = f(lock).await;

    // Because we sent value as Arc, it may still be in use.
    // Wait for signal saying it has been dropped.
    // Note: we must not be holding the lock value here!
    rx.await?;

    for repo in &clone {
        helm::remove_repo(repo).await?;
    }

    Ok(rc)
}

// Define the possible results of a job.
enum JobResult {
    Unit,                   // Represents a unit result.
    Diff(helm::DiffResult), // Represents a diff result.
}

// Asynchronously run a job based on the provided command.
async fn run_job(
    command: &Request,                // The command to execute.
    helm_repos: &HelmReposLock,       // The helm repositories lock.
    installation: &Arc<Installation>, // The installation details.
    tx: &MultiOutput,                 // The multi-output channel.
) -> Result<JobResult> {
    match command {
        // Handle the Upgrade request.
        Request::Upgrade { .. } => {
            // Run the helm diff command.
            let diff_result = helm::diff(installation, helm_repos, tx).await?;
            // Check the exit code of the diff command.
            if diff_result._exit_code == 0 || diff_result._exit_code == 2 {
                // If no changes or only changes detected, return Unit.
                return Ok(JobResult::Unit);
            }
            // Perform a dry-run upgrade.
            helm::upgrade(installation, helm_repos, tx, true).await?;
            // Perform the actual upgrade.
            helm::upgrade(installation, helm_repos, tx, false).await?;
            // Return Unit result.
            Ok(JobResult::Unit)
        }
        // Handle the Diff request.
        Request::Diff { .. } => {
            // Run the helm diff command.
            let diff_result = helm::diff(installation, helm_repos, tx).await?;
            // Return the diff result.
            Ok(JobResult::Diff(helm::DiffResult {
                _exit_code: diff_result._exit_code,
            }))
        }
        Request::Test { .. } => {
            helm::outdated(installation, helm_repos, tx).await?;
            helm::lint(installation, tx).await?;
            helm::template(installation, helm_repos, tx).await?;
            Ok(JobResult::Unit)
        }
        Request::Template { .. } => {
            helm::template(installation, helm_repos, tx).await?;
            Ok(JobResult::Unit)
        }
        Request::Outdated { .. } => {
            helm::outdated(installation, helm_repos, tx).await?;
            Ok(JobResult::Unit)
        }
        Request::Update { updates, .. } => {
            helm::update(installation, tx, updates).await?;
            Ok(JobResult::Unit)
        }
    }
}

#[derive(Copy, Clone, clap::ValueEnum, Debug, Eq, PartialEq)]
enum OutputFormat {
    /// Use gitlab compliant text + slack output.
    Text,
    /// Use full screen TUI interface.
    Tui,
    /// Use slack output.
    Slack,
}

#[derive(Clone, clap::ValueEnum, Debug)]
enum AutoState {
    /// Only process releases with auto==true.
    Yes,
    /// Process all releases regardless of auto value.
    All,
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = "Program to automate CI deploys")]
struct Args {
    #[clap(subcommand)]
    command: Request,

    /// Filter releases to use based on env.
    #[clap(long, default_value = "*")]
    env: String,

    /// Filter releases to use based on cluster.
    #[clap(long)]
    cluster: Vec<String>,

    /// The source directory containing the helm-values.
    #[clap(long)]
    vdir: String,

    /// Provide an override file to use.
    #[clap(long)]
    overrides: Option<String>,

    /// What method should be use to display output?
    #[clap(long, value_enum)]
    output: Vec<OutputFormat>,

    #[clap(long)]
    release_filter: Vec<ReleaseFilter>,

    /// Should we process releases that have auto set to false?
    #[clap(long, value_enum, default_value_t=AutoState::Yes)]
    auto: AutoState,
}

#[derive(Subcommand, Debug, Clone)]
enum Request {
    /// Upgrade/install releases.
    Upgrade {},

    /// Diff releases with current state.
    Diff {},

    /// Test releases.
    Test {},

    /// Generate template of releases.
    Template {},

    /// Generate outdated report of releases.
    Outdated {},

    /// Update helm charts.
    Update {
        /// List of changes
        updates: Vec<Update>,
    },
}

#[derive(Clone, Debug)]
enum ReleaseFilter {
    ChartType(String),
    ChartName(String),
    ReleaseName(String),
}

impl FromStr for ReleaseFilter {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.splitn(2, '=');
        let key = split
            .next()
            .ok_or_else(|| anyhow!("invalid filter: {}", s))?
            .to_string();
        let value = split
            .next()
            .ok_or_else(|| anyhow!("invalid filter: {}", s))?
            .to_string();
        match key.as_str() {
            "chart_type" => Ok(ReleaseFilter::ChartType(value)),
            "chart_name" => Ok(ReleaseFilter::ChartName(value)),
            "release_name" => Ok(ReleaseFilter::ReleaseName(value)),
            _ => Err(anyhow!("invalid filter key: {}", key)),
        }
    }
}

impl ReleaseFilter {
    fn matches(&self, release: &Release) -> bool {
        #[allow(clippy::match_same_arms)]
        match self {
            Self::ChartType(chart_type) => match &release.config.release_chart {
                ChartReference::Helm { .. } => chart_type == "helm",
                ChartReference::Oci { .. } => chart_type == "oci",
                ChartReference::Local { .. } => chart_type == "local",
            },
            Self::ChartName(chart_name) => match &release.config.release_chart {
                ChartReference::Helm {
                    chart_name: name, ..
                } => chart_name == name,
                ChartReference::Oci {
                    chart_name: name, ..
                } => chart_name == name,
                ChartReference::Local { .. } => false,
            },
            Self::ReleaseName(release_name) => release.name == *release_name,
        }
    }
}

impl Request {
    const fn do_depends(&self) -> bool {
        matches!(self, Self::Upgrade { .. })
    }

    const fn requires_helm_repos(&self) -> bool {
        #![allow(clippy::match_same_arms)]
        match self {
            Self::Upgrade { .. } => true,
            Self::Diff { .. } => true,
            Self::Test { .. } => true,
            Self::Template { .. } => true,
            Self::Outdated { .. } => true,
            Self::Update { .. } => false,
        }
    }
}

fn get_output(output_format: OutputFormat) -> Result<(Box<dyn Output>, Sender)> {
    let output: (Box<dyn Output>, Sender) = match output_format {
        OutputFormat::Text => {
            let (a, b) = output::text::start();
            (Box::new(a), b)
        }
        OutputFormat::Slack => {
            let (a, b) = output::slack::start().context("Cannot start slack")?;
            (Box::new(a), b)
        }
        OutputFormat::Tui => {
            let (a, b) = output::tui::start().context("Cannot start TUI")?;
            (Box::new(a), b)
        }
    };
    Ok(output)
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "helmci=debug");
    }

    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let output_types = if args.output.is_empty() {
        vec![OutputFormat::Text]
    } else {
        args.output.clone()
    };

    if output_types.contains(&OutputFormat::Tui) && args.output.contains(&OutputFormat::Text) {
        return Err(anyhow::anyhow!(
            "Cannot use both TUI and Text output together"
        ));
    }

    let (mut outputs, output_pipe) = {
        let len = output_types.len();
        let mut outputs = Vec::with_capacity(len);
        let mut output_pipes = Vec::with_capacity(len);

        for output_type in output_types {
            let (output, tx) = get_output(output_type)?;
            outputs.push(output);
            output_pipes.push(tx);
        }

        let output_pipe = MultiOutput::new(output_pipes);
        (outputs, output_pipe)
    };

    // Note this will only setup logging for current task, other tasks will use default
    // global logging setup above.
    let filter = tracing_subscriber::filter::EnvFilter::from_default_env();
    let guard = filter
        .and_then(CustomLayer::new(output_pipe.clone()))
        .with_subscriber(Registry::default())
        .set_default();

    let command = Arc::new(args.command.clone());

    // Send the Start message to output.
    let start = Instant::now();
    output_pipe
        .send(Message::Start(command.clone(), start))
        .await;

    // Save the error for now so we can clean up.
    let rc = do_task(command, &args, &output_pipe).await;

    // Log the error.
    if let Err(err) = &rc {
        error!("ERROR: {}", err);
        err.chain()
            .skip(1)
            .for_each(|cause| error!("because: {}", cause));
    }

    // Send the FinishedAll message to output.
    let result_to_send = match &rc {
        Ok(()) => Ok(()),
        Err(_) => Err("Tasks had errors".to_string()),
    };
    let stop = Instant::now();
    output_pipe
        .send(output::Message::FinishedAll(result_to_send, stop - start))
        .await;

    // We have to unconfigure logging here (drop guard) so we can release the output handle.
    drop(output_pipe);
    drop(guard);

    for output in &mut outputs {
        output.wait().await.context("The output plugin failed")?;
    }

    // Exit with any error we saved above.
    rc
}

async fn do_task(command: Arc<Request>, args: &Args, output: &output::MultiOutput) -> Result<()> {
    // let mut helm_repos = HelmRepos::new();

    let (skipped_list, todo) = generate_todo(args)?;

    let mut skipped = InstallationSet::new();
    for item in skipped_list {
        skipped.add(&item);
        output.send(Message::SkippedJob(item)).await;
    }

    // let jobs: Jobs = (command, todo);
    run_jobs_concurrently(command, todo, output, skipped).await
}

type SkippedResult = Arc<Installation>;

#[allow(clippy::cognitive_complexity)]
fn generate_todo(
    args: &Args,
) -> Result<(Vec<SkippedResult>, Vec<Arc<Installation>>), anyhow::Error> {
    let vdir = PathBuf::from(&args.vdir);
    let envs = Env::list_all_env(&vdir)
        .with_context(|| format!("Cannot list envs from vdir {}", vdir.display()))?;
    let mut todo: Vec<Arc<Installation>> = Vec::new();
    let mut skipped: Vec<SkippedResult> = Vec::new();
    let mut seen = InstallationSet::default();
    let mut next_id: InstallationId = 0;

    let overrides = match &args.overrides {
        Some(path) => {
            let override_path = PathBuf::from(path);
            Overrides::load(&override_path)?
        }
        None => Overrides::default(),
    };

    for env_name in envs {
        let env =
            Env::load(&vdir, &env_name).with_context(|| format!("Cannot load env {env_name}"))?;

        if env.config.locked {
            warn!("Skipping locked env {}", env.name);
            continue;
        }

        if args.env != "*" && args.env != env_name {
            trace!("Skipping env {}", env.name);
            continue;
        }

        trace!("Processing env {}", env.name);

        let all_clusters = env
            .list_all_clusters()
            .with_context(|| format!("Cannot list clusters from env {env_name}"))?;
        for cluster_name in all_clusters {
            let cluster = env.load_cluster(&cluster_name).with_context(|| {
                format!("Cannot load cluster {cluster_name} from env {env_name}")
            })?;

            if cluster.config.locked {
                warn!("Skipping locked cluster {}", cluster.name);
                continue;
            }

            if !args.cluster.is_empty() && !args.cluster.contains(&cluster_name) {
                trace!("Skipping cluster {}", cluster.name);
                continue;
            }

            trace!("Processing cluster {}", cluster.name);

            let all_releases = cluster.list_all_releases().with_context(|| {
                format!("Cannot list releases from cluster {cluster_name} env {env_name}")
            })?;
            for release_dir_name in &all_releases {
                let release = cluster
                    .load_release(release_dir_name, &overrides)
                    .with_context(|| {
                        format!("Cannot load releases from cluster {cluster_name} env {env_name}")
                    })?;
                trace!("Processing install {}", release.name);

                let auto_skip = match args.auto {
                    AutoState::Yes => !release.config.auto,
                    AutoState::All => false,
                };

                // We also do skip entries if the install is to be skipped, these will be shown
                let skip = args
                    .release_filter
                    .iter()
                    .any(|filter| !filter.matches(&release))
                    || release.config.locked
                    || auto_skip;

                let installation = create_installation(&env, &cluster, release, next_id);
                next_id = installation.id + 1;

                if seen.contains(&installation) {
                    anyhow::bail!(
                        "There is a conflicting installation: {} {} {}",
                        installation.context,
                        installation.namespace,
                        installation.name
                    );
                }
                seen.add(&installation);

                // Note: skipped installs count towards dependency requirements
                let installation = Arc::new(installation);
                if skip {
                    skipped.push(installation);
                } else {
                    todo.push(installation);
                }
            }
        }
    }
    Ok((skipped, todo))
}

fn create_installation(
    env: &Env,
    cluster: &Cluster,
    release: Release,
    id: InstallationId,
) -> Installation {
    let depends = release.config.depends.unwrap_or_default();

    let mut values_files: Vec<ValuesFile> = vec![];

    for base_values_file in release.config.base_values_files {
        let path = release.dir.join(&base_values_file.path);
        let values_file = ValuesFile {
            path,
            format: base_values_file.format,
        };
        values_files.push(values_file);
    }

    let values_file = release.dir.join("values.yaml");
    if values_file.is_file() {
        values_files.push(ValuesFile {
            path: values_file,
            format: ValuesFormat::PlainText,
        });
    }
    let secrets_file = release.dir.join("values.secrets");
    if secrets_file.is_file() {
        values_files.push(ValuesFile {
            path: secrets_file,
            format: ValuesFormat::PlainText,
        });
    }

    for override_values_file in release.config.override_values_files {
        let path = release.dir.join(&override_values_file.path);
        let values_file = ValuesFile {
            path,
            format: override_values_file.format,
        };
        values_files.push(values_file);
    }

    // Turn legacy config into new config
    let chart_reference = release.config.release_chart;

    let announce_policy = release
        .config
        .announce_policy
        .unwrap_or(config::AnnouncePolicy::None);

    Installation {
        name: release.name,
        config_file: release.config_file,
        namespace: release.config.namespace,
        env_name: env.name.clone(),
        cluster_name: cluster.name.clone(),
        context: cluster.config.context.clone(),
        values_files,
        chart_reference,
        depends,
        timeout: release.config.timeout,
        id,
        announce_policy,
    }
}

const NUM_THREADS: usize = 6;

#[derive(Debug)]
enum Dispatch {
    RequestNextJob(oneshot::Sender<Option<Arc<Installation>>>),
    Done(HashIndex),
}

fn is_ok(do_depends: bool, some_i: Option<&Arc<Installation>>, done: &InstallationSet) -> bool {
    some_i.map_or(false, |i| !do_depends || is_depends_ok(i, done))
}

async fn run_jobs_concurrently(
    request: Arc<Request>,
    todo: Vec<Arc<Installation>>,
    output: &output::MultiOutput,
    skipped: InstallationSet,
) -> Result<()> {
    let required_repos = if request.requires_helm_repos() {
        get_required_repos(&todo)
    } else {
        vec![]
    };

    let rc = with_helm_repos(required_repos, |repos| async {
        run_jobs_concurrently_with_repos(request, todo, output, skipped, repos).await
    })
    .await;

    rc?
}

async fn run_jobs_concurrently_with_repos(
    request: Arc<Request>,
    todo: Vec<Arc<Installation>>,
    output: &output::MultiOutput,
    skipped: InstallationSet,
    helm_repos: Arc<HelmReposLock>,
) -> Result<()> {
    let do_depends = request.do_depends();
    // let skip_depends = !matches!(jobs.0, Task::Upgrade | Task::Test);
    // let task = jobs.0.clone();

    for i in &todo {
        output.send(Message::NewJob(i.clone())).await;
    }

    // This process receives requests for jobs and dispatches them
    let (tx_dispatch, rx_dispatch) = mpsc::channel(10);
    let dispatch =
        tokio::spawn(async move { dispatch_thread(todo, skipped, rx_dispatch, do_depends).await });

    // The actual worker threads
    let threads: Vec<JoinHandle<Result<()>>> = (0..NUM_THREADS)
        .map(|_| {
            let tx_dispatch = tx_dispatch.clone();
            let output = output.clone();
            let request = request.clone();
            let helm_repos = helm_repos.clone();
            tokio::spawn(async move {
                worker_thread(&request, &helm_repos, &tx_dispatch, &output).await
            })
        })
        .collect();

    drop(tx_dispatch);

    let mut errors: bool = false;
    for t in threads {
        trace!("Waiting for worker thread");
        let rc = t.await;

        match rc {
            Ok(Ok(())) => trace!("Worker thread finished without errors"),
            Ok(Err(err)) => {
                error!("Worker thread returned error: {err}");
                errors = true;
            }
            Err(err) => {
                error!("Error waiting for worker thread: {err}");
                errors = true;
            }
        }
    }

    trace!("Waiting for dispatch thread");
    let rc = dispatch.await;

    match rc {
        Ok(Ok(())) => {
            trace!("Dispatch finished without errors");
            Ok(())
        }
        Ok(Err(err)) => {
            error!("Dispatch thread returned error: {err}");
            Err(err)
        }
        Err(err) => {
            error!("Error waiting for dispatch thread: {err}");
            Err(anyhow!("Dispatch thread join error: {err}"))
        }
    }?;

    if errors {
        Err(anyhow::anyhow!("Errors were detected"))
    } else {
        Ok(())
    }
}

async fn dispatch_thread(
    todo: Vec<Arc<Installation>>,
    skipped: InstallationSet,
    mut rx_dispatch: mpsc::Receiver<Dispatch>,
    do_depends: bool,
) -> Result<(), anyhow::Error> {
    let mut installations: Vec<Option<&Arc<Installation>>> = todo.iter().map(Some).collect();
    let mut done = skipped;
    while let Some(msg) = rx_dispatch.recv().await {
        match msg {
            Dispatch::RequestNextJob(tx) => {
                // Search for first available installation that meets dependancies.
                let some_i = installations
                    .iter_mut()
                    .find(|i| is_ok(do_depends, **i, &done))
                    .and_then(std::option::Option::take)
                    .map(std::borrow::ToOwned::to_owned);

                // Send it back to requestor
                tx.send(some_i)
                    .map_err(|_err| anyhow!("Dispatch: Cannot send response"))?;
            }
            Dispatch::Done(hash) => {
                done.add_hash(hash);
            }
        }
    }
    let remaining: Vec<_> = installations.iter().filter(|i| i.is_some()).collect();
    if remaining.is_empty() {
        Ok(())
    } else {
        remaining
            .iter()
            .filter_map(|i| **i)
            .for_each(|i| error!("Still pending {:?}", i.name));
        Err(anyhow::anyhow!(
            "Installations still pending; probably broken dependancies"
        ))
    }
}

async fn worker_thread(
    command: &Request,
    helm_repos: &HelmReposLock,
    tx_dispatch: &mpsc::Sender<Dispatch>,
    output: &MultiOutput,
) -> Result<()> {
    let mut errors = false;

    loop {
        // Get the next available job
        let (tx_response, rx_response) = oneshot::channel();
        tx_dispatch
            .send(Dispatch::RequestNextJob(tx_response))
            .await?;
        let some_i = rx_response.await?;

        let Some(install) = some_i else { break };

        // Update UI
        let start = Instant::now();
        output
            .send(Message::StartedJob(install.clone(), start))
            .await;

        // Execute the job
        let result = run_job(command, helm_repos, &install, output).await;
        match &result {
            Ok(JobResult::Unit) => {
                // Handle the unit case
                tx_dispatch
                    .send(Dispatch::Done(HashIndex::get_hash_index(&install)))
                    .await?;
            }
            Ok(JobResult::Diff(helm::DiffResult { _exit_code })) => {
                // Handle the diff result case
                // You might want to log or process the diff_result here
                tx_dispatch
                    .send(Dispatch::Done(HashIndex::get_hash_index(&install)))
                    .await?;
            }
            Err(err) => {
                output
                    .send(Message::Log(log!(
                        Level::ERROR,
                        &format!("job failed: {err}")
                    )))
                    .await;
                errors = true;
            }
        }

        // Update UI
        let stop = Instant::now();
        output
            .send(Message::FinishedJob(
                install.clone(),
                result.map(|_| ()).map_err(|err| err.to_string()),
                stop - start,
            ))
            .await;
    }

    if errors {
        Err(anyhow::anyhow!("This thread received errors"))
    } else {
        Ok(())
    }
}
