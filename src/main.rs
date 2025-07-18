// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.

//! The main helmci program code
#![warn(missing_docs)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::use_self)]

use std::path::PathBuf;
use std::str::{self, FromStr};
use std::sync::Arc;

use anyhow::Result;
use anyhow::{Context, anyhow};

use clap::Parser;
use clap::Subcommand;

use repos::cache::Cache;
use repos::locks::Lock;
use repos::{Repos, download_by_reference};
use tap::Pipe;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

mod command;

mod helm;
use helm::Installation;
use helm::InstallationId;
use helm::{DiffResult, DownloadedInstallation};

mod depends;
use depends::{HashIndex, InstallationSet, is_depends_ok};

mod output;
use output::{JobSuccess, Message, MultiOutput, Output, Sender};
use output::{error, info, trace, warning};

mod config;
use config::Release;
use config::{ChartReference, Cluster, Env};
use config::{ValuesFile, ValuesFormat};

mod logging;

mod duration;

mod utils;

mod repos;

mod versions;

mod urls;

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

async fn get_required_repos(todo: &[Arc<Installation>]) -> Result<Repos> {
    let mut repos = Repos::new();

    for installation in todo {
        repos.download(&installation.chart_reference).await?;
    }

    Ok(repos)
}

async fn run_job(
    command: &Request,
    installation: &Arc<DownloadedInstallation>,
    tx: &MultiOutput,
) -> Result<JobSuccess> {
    match command {
        Request::Upgrade {
            skip_upgrade_on_no_changes,
        } => {
            helm::lint(installation, tx).await?;
            let diff_result = helm::diff(installation, tx).await?;
            match (diff_result, skip_upgrade_on_no_changes) {
                (DiffResult::Changes, _) | (_, false) => {
                    helm::upgrade(installation, tx, true).await?;
                    helm::upgrade(installation, tx, false).await?;
                    Ok(JobSuccess::Completed)
                }
                (DiffResult::NoChanges, true) => Ok(JobSuccess::Skipped),
            }
        }
        Request::Diff { .. } => {
            helm::diff(installation, tx).await?;
            Ok(JobSuccess::Completed)
        }
        Request::Test { .. } => {
            helm::outdated(installation, tx).await?;
            helm::lint(installation, tx).await?;
            helm::template(installation, tx).await?;
            Ok(JobSuccess::Completed)
        }
        Request::Template { .. } => {
            helm::template(installation, tx).await?;
            Ok(JobSuccess::Completed)
        }
        Request::Outdated { .. } => {
            helm::outdated(installation, tx).await?;
            Ok(JobSuccess::Completed)
        }
        Request::Update { updates, .. } => {
            helm::update(installation, tx, updates).await?;
            Ok(JobSuccess::Completed)
        }
        Request::RewriteLocks => {
            // This requires access to cache directory
            // which cannot be done concurrently so
            // this not done here.
            Ok(JobSuccess::Skipped)
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
    vdir: PathBuf,

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
    Upgrade {
        /// Skip upgrade on no changes.
        #[clap(long, short = 'b', default_value_t = true, num_args = 1)]
        skip_upgrade_on_no_changes: bool,
    },

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

    /// Rewrite lock files.
    RewriteLocks,
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
            Self::Outdated { .. } => false,
            Self::Update { .. } => false,
            Self::RewriteLocks { .. } => false,
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
    if let Err(err) = rustls::crypto::aws_lc_rs::default_provider().install_default() {
        return Err(anyhow::anyhow!(
            "Failed to install rustls crypto provider: {err:?}"
        ));
    }

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
        error!(output_pipe, "ERROR: {}", err).await;
        let err_list = err.chain().skip(1);
        for cause in err_list {
            error!(output_pipe, "because: {}", cause).await;
        }
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
    for output in &mut outputs {
        output.wait().await.context("The output plugin failed")?;
    }

    // Exit with any error we saved above.
    rc
}

async fn do_task(command: Arc<Request>, args: &Args, output: &output::MultiOutput) -> Result<()> {
    let (skipped_list, todo) = generate_todo(args, output).await?;

    let mut skipped = InstallationSet::new();
    for item in skipped_list {
        skipped.add(&item);
        output.send(Message::SkippedJob(item)).await;
    }

    let cache = Cache::new(&args.vdir);
    run_jobs_concurrently(command, cache, todo, output, skipped).await
}

type SkippedResult = Arc<Installation>;

#[allow(clippy::cognitive_complexity)]
async fn generate_todo(
    args: &Args,
    output: &output::MultiOutput,
) -> Result<(Vec<SkippedResult>, Vec<Arc<Installation>>), anyhow::Error> {
    let vdir = PathBuf::from(&args.vdir);
    let envs = Env::list_all_env(&vdir)
        .with_context(|| format!("Cannot list envs from vdir {}", vdir.display()))?;
    let mut todo: Vec<Arc<Installation>> = Vec::new();
    let mut skipped: Vec<SkippedResult> = Vec::new();
    let mut seen = InstallationSet::default();
    let mut next_id: InstallationId = 0;

    for env_name in envs {
        let env =
            Env::load(&vdir, &env_name).with_context(|| format!("Cannot load env {env_name}"))?;

        if env.config.locked {
            warning!(output, "Skipping locked env {}", env.name).await;
            continue;
        }

        if args.env != "*" && args.env != env_name {
            trace!(output, "Skipping env {}", env.name).await;
            continue;
        }

        trace!(output, "Processing env {}", env.name).await;

        let all_clusters = env
            .list_all_clusters()
            .with_context(|| format!("Cannot list clusters from env {env_name}"))?;
        for cluster_name in all_clusters {
            let cluster = env.load_cluster(&cluster_name).with_context(|| {
                format!("Cannot load cluster {cluster_name} from env {env_name}")
            })?;

            if cluster.config.locked {
                warning!(output, "Skipping locked cluster {}", cluster.name).await;
                continue;
            }

            if !args.cluster.is_empty() && !args.cluster.contains(&cluster_name) {
                trace!(output, "Skipping cluster {}", cluster.name).await;
                continue;
            }

            trace!(output, "Processing cluster {}", cluster.name).await;

            let all_releases = cluster.list_all_releases().with_context(|| {
                format!("Cannot list releases from cluster {cluster_name} env {env_name}")
            })?;
            for release_dir_name in &all_releases {
                let release = cluster.load_release(release_dir_name).with_context(|| {
                    format!("Cannot load releases from cluster {cluster_name} env {env_name}")
                })?;
                trace!(output, "Processing install {}", release.name).await;

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
    let sops_file = release.dir.join("sops.yaml");
    if sops_file.is_file() {
        values_files.push(ValuesFile {
            path: sops_file,
            format: ValuesFormat::Sops,
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
        lock_file: release.lock_file,
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
    Done(HashIndex),
    Errored(HashIndex),
}

fn is_ok(do_depends: bool, some_i: Option<&Arc<Installation>>, done: &InstallationSet) -> bool {
    some_i.is_some_and(|i| !do_depends || is_depends_ok(i, done))
}

async fn run_jobs_concurrently(
    request: Arc<Request>,
    cache: Cache,
    todo: Vec<Arc<Installation>>,
    output: &output::MultiOutput,
    skipped: InstallationSet,
) -> Result<()> {
    let downloaded_installations = if request.requires_helm_repos() {
        info!(output, "Downloading repos for installation").await;
        let repos = get_required_repos(&todo).await?;

        let mut downloaded = Vec::with_capacity(todo.len());
        for installation in &todo {
            let lock = Lock::load(&installation.lock_file)?;
            let meta = lock.meta;
            info!(
                output,
                "Downloading chart {} using {:?}", installation.chart_reference, meta
            )
            .await;
            let chart =
                repos::download_by_meta(&repos, &cache, &meta, &installation.chart_reference)
                    .await
                    .map_err(|e| {
                        anyhow!(
                            "Failed to download chart for installation: {} - {}",
                            installation.name,
                            e
                        )
                    })?;
            DownloadedInstallation {
                installation: installation.clone(),
                chart: Some(chart),
            }
            .pipe(Arc::new)
            .pipe(|x| downloaded.push(x));
        }
        downloaded
    } else {
        let mut downloaded = Vec::with_capacity(todo.len());
        for installation in &todo {
            DownloadedInstallation {
                installation: installation.clone(),
                chart: None,
            }
            .pipe(Arc::new)
            .pipe(|x| downloaded.push(x));
        }
        downloaded
    };

    if !matches!(*request, Request::RewriteLocks) {
        run_jobs_concurrently_with_repos(
            request.clone(),
            downloaded_installations,
            output,
            skipped,
        )
        .await?;
    }

    if matches!(*request, Request::RewriteLocks | Request::Update { .. }) {
        info!(output, "Downloading repos for updating locks").await;
        let repos = get_required_repos(&todo).await?;

        for installation in todo {
            let lock_file = &installation.lock_file;

            info!(output, "Rewriting lock file {}", lock_file.display()).await;
            let old = if lock_file.exists() {
                Some(Lock::load(lock_file)?)
            } else {
                None
            };

            let chart_reference = get_new_chart_reference(&request, &installation)?;
            if let Some(chart_reference) = chart_reference {
                info!(output, "Downloading chart {:?}", chart_reference).await;
                let chart = download_by_reference(&repos, &cache, &chart_reference).await?;
                let new = Lock::new(chart.meta);

                if let Some(old) = old {
                    if old.meta == new.meta {
                        info!(output, "No change to lock file {}", lock_file.display()).await;
                    } else {
                        info!(output, "Updated lock file {}", lock_file.display()).await;
                        new.save(lock_file)?;
                    }
                } else {
                    info!(output, "Created lock file {}", lock_file.display()).await;
                    new.save(lock_file)?;
                }
            }
        }

        return Ok(());
    }

    Ok(())
}

fn get_new_chart_reference(
    request: &Request,
    installation: &Installation,
) -> Result<Option<ChartReference>, anyhow::Error> {
    let chart_reference = match request {
        Request::Update { .. } => {
            // Reload the release file because it may have changed

            let new_chart_reference = serde_yml::from_str::<config::ReleaseConfig>(
                &std::fs::read_to_string(&installation.config_file)?,
            )
            .context("Failed to parse config file")?
            .release_chart;

            if installation.chart_reference == new_chart_reference {
                None
            } else {
                Some(new_chart_reference)
            }
        }
        Request::RewriteLocks => Some(installation.chart_reference.clone()),
        _ => None,
    };
    Ok(chart_reference)
}

async fn run_jobs_concurrently_with_repos(
    request: Arc<Request>,
    todo: Vec<Arc<DownloadedInstallation>>,
    output: &output::MultiOutput,
    skipped: InstallationSet,
) -> Result<()> {
    let do_depends = request.do_depends();
    // let skip_depends = !matches!(jobs.0, Task::Upgrade | Task::Test);
    // let task = jobs.0.clone();

    for i in &todo {
        output.send(Message::NewJob(i.installation.clone())).await;
    }

    // This process receives requests for jobs and dispatches them
    let (tx_job, rx_job) = async_channel::unbounded();
    let (tx_dispatch, rx_dispatch) = mpsc::channel(10);
    let output_clone = output.clone();
    let dispatch = tokio::spawn(async move {
        dispatch_thread(
            todo,
            skipped,
            tx_job,
            rx_dispatch,
            do_depends,
            &output_clone,
        )
        .await
    });

    // The actual worker threads
    let threads: Vec<JoinHandle<Result<()>>> = (0..NUM_THREADS)
        .map(|_| {
            let rx_job = rx_job.clone();
            let tx_dispatch = tx_dispatch.clone();
            let output = output.clone();
            let request = request.clone();
            tokio::spawn(
                async move { worker_thread(&request, &rx_job, &tx_dispatch, &output).await },
            )
        })
        .collect();

    drop(tx_dispatch);

    let mut errors: bool = false;
    for t in threads {
        trace!(output, "Waiting for worker thread").await;
        let rc = t.await;

        match rc {
            Ok(Ok(())) => trace!(output, "Worker thread finished without errors").await,
            Ok(Err(err)) => {
                error!(output, "Worker thread returned error: {err}").await;
                errors = true;
            }
            Err(err) => {
                error!(output, "Error waiting for worker thread: {err}").await;
                errors = true;
            }
        }
    }

    trace!(output, "Waiting for dispatch thread").await;
    let rc = dispatch.await;

    match rc {
        Ok(Ok(())) => {
            trace!(output, "Dispatch finished without errors").await;
            Ok(())
        }
        Ok(Err(err)) => {
            error!(output, "Dispatch thread returned error: {err}").await;
            Err(err)
        }
        Err(err) => {
            error!(output, "Error waiting for dispatch thread: {err}").await;
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
    todo: Vec<Arc<DownloadedInstallation>>,
    skipped: InstallationSet,
    tx_job: async_channel::Sender<Arc<DownloadedInstallation>>,
    mut rx_dispatch: mpsc::Receiver<Dispatch>,
    do_depends: bool,
    output: &MultiOutput,
) -> Result<(), anyhow::Error> {
    let mut installations: Vec<Option<&Arc<DownloadedInstallation>>> =
        todo.iter().map(Some).collect();
    let mut waiting = InstallationSet::new();
    let mut done = skipped;
    loop {
        // Get pending installations
        let pending = installations
            .iter_mut()
            .filter(|i| is_ok(do_depends, i.map(|i| &i.installation), &done))
            .filter_map(Option::take)
            .map(std::borrow::ToOwned::to_owned);

        // Dispatch pending installations
        for p in pending {
            info!(output, "dispatching {}", p.installation.name).await;
            waiting.add(&p.installation);
            if let Err(_err) = tx_job.send(p).await {
                break;
            }
        }

        // Exit if there are no running jobs
        if waiting.is_empty() {
            break;
        }

        // Wait for installations
        match rx_dispatch.recv().await {
            Some(Dispatch::Done(hash)) => {
                waiting.remove_hash(&hash);
                done.add_hash(hash);
            }
            Some(Dispatch::Errored(hash)) => {
                waiting.remove_hash(&hash);
            }
            None => break,
        }
    }
    tx_job.close();
    let remaining: Vec<_> = installations.iter().flatten().collect();
    if !waiting.is_empty() {
        error!(output, "Exited but still waiting on jobs").await;
        Err(anyhow::anyhow!("Still waiting on jobs"))
    } else if remaining.is_empty() {
        Ok(())
    } else {
        for i in remaining {
            error!(output, "Still pending {:?}", i.installation.name).await;
        }
        Err(anyhow::anyhow!(
            "Installations still pending; probably broken dependancies"
        ))
    }
}

async fn worker_thread(
    command: &Request,
    rx_job: &async_channel::Receiver<Arc<DownloadedInstallation>>,
    tx_dispatch: &mpsc::Sender<Dispatch>,
    output: &MultiOutput,
) -> Result<()> {
    let mut errors = false;

    loop {
        let install = match rx_job.recv().await {
            Ok(install) => install,
            Err(_err) => break,
        };

        // Update UI
        let start = Instant::now();
        output
            .send(Message::StartedJob(install.installation.clone(), start))
            .await;

        // Execute the job
        let result = run_job(command, &install, output).await;
        match &result {
            Ok(_) => {
                tx_dispatch
                    .send(Dispatch::Done(HashIndex::get_hash_index(
                        &install.installation,
                    )))
                    .await?;
            }
            Err(err) => {
                tx_dispatch
                    .send(Dispatch::Errored(HashIndex::get_hash_index(
                        &install.installation,
                    )))
                    .await?;
                error!(output, "job failed: {err}").await;
                errors = true;
            }
        }

        // Update UI
        let stop = Instant::now();
        output
            .send(Message::FinishedJob(
                install.installation.clone(),
                result.map_err(|err| err.to_string()),
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
