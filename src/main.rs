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

use std::collections::hash_map::Iter;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::str;
use std::sync::Arc;

use anyhow::Result;
use anyhow::{anyhow, Context};

use clap::Parser;
use clap::Subcommand;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing_subscriber::util::SubscriberInitExt;

use tracing::{error, trace, warn, Level};
use tracing_subscriber::{Layer, Registry};

mod command;

mod helm;
use helm::{HelmChart, Installation, ValuesFile, ValuesFormat};
use helm::{HelmRepo, InstallationId};

mod depends;
use depends::{is_depends_ok, HashIndex, InstallationSet};

mod output;
use output::{Message, MultiOutput, Output, Sender};

mod config;
use config::Overrides;
use config::Release;
use config::{ChartReference, Cluster, Env};

mod layer;
use layer::log;
use layer::CustomLayer;

mod duration;

mod utils;

/// What task was requested?
#[derive(Copy, Clone, Debug)]
pub enum Task {
    /// Helm upgrade requested.
    Upgrade,
    /// Helm diff report requested.
    Diff,
    /// Helm tests requested.
    Test,
    /// Helm template requested.
    Template,
    /// Helm outdated report requested.
    Outdated,
}

type Jobs = (Task, Vec<Arc<Installation>>);

fn get_required_repos(jobs: &Jobs, helm_repos: &HelmRepos) -> Vec<HelmRepo> {
    let mut names = HashSet::new();

    for installation in &jobs.1 {
        if let HelmChart::HelmRepo { repo, .. } = &installation.chart {
            names.insert(&repo.name);
        }
    }

    // Add these repos
    let mut repos: Vec<HelmRepo> = Vec::with_capacity(names.len());
    for repo in helm_repos.iter() {
        if names.contains(&repo.name) {
            repos.push(repo);
        }
    }

    repos
}

async fn helm_add_repos(repos: &[HelmRepo]) -> Result<()> {
    // Add these repos
    for repo in repos {
        helm::add_repo(repo).await?;
    }

    Ok(())
}

async fn helm_remove_repos_for_jobs(repos: &[HelmRepo]) -> Result<()> {
    // Add these repos
    for repo in repos {
        helm::remove_repo(repo).await?;
    }

    Ok(())
}

async fn run_job(task: Task, installation: &Arc<Installation>, tx: &MultiOutput) -> Result<()> {
    match task {
        Task::Upgrade => {
            helm::upgrade(installation, tx, true).await?;
            helm::upgrade(installation, tx, false).await
        }
        Task::Diff => helm::diff(installation, tx).await,
        Task::Test => {
            helm::outdated(installation, tx).await?;
            helm::lint(installation, tx).await?;
            helm::template(installation, tx).await
        }
        Task::Template => helm::template(installation, tx).await,
        Task::Outdated => helm::outdated(installation, tx).await,
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
    command: Commands,

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

    /// Should we process releases that have auto set to false?
    #[clap(long, value_enum, default_value_t=AutoState::Yes)]
    auto: AutoState,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Upgrade/install releases.
    Upgrade {
        #[clap()]
        charts: Vec<String>,
    },

    /// Diff releases with current state.
    Diff {
        #[clap()]
        charts: Vec<String>,
    },

    /// Test releases.
    Test {
        #[clap()]
        charts: Vec<String>,
    },

    /// Generate template of releases.
    Template {
        #[clap()]
        charts: Vec<String>,
    },

    /// Generate outdated report of releases.
    Outdated {
        #[clap()]
        charts: Vec<String>,
    },
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

    // Work out what task we need to do.
    let (task, installs) = match &args.command {
        Commands::Upgrade { charts: installs } => (Task::Upgrade, installs),
        Commands::Diff { charts: installs } => (Task::Diff, installs),
        Commands::Test { charts: installs } => (Task::Test, installs),
        Commands::Template { charts: installs } => (Task::Template, installs),
        Commands::Outdated { charts: installs } => (Task::Outdated, installs),
    };

    // Send the Start message to output.
    let start = Instant::now();
    output_pipe.send(Message::Start(task, start)).await;

    // Save the error for now so we can clean up.
    let rc = do_task(task, &args, installs, &output_pipe).await;

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

async fn do_task(
    task: Task,
    args: &Args,
    list_release_names: &[String],
    output: &output::MultiOutput,
) -> Result<()> {
    let mut helm_repos = HelmRepos::new();

    let (skipped_list, todo) = generate_todo(args, list_release_names, &mut helm_repos)?;

    let mut skipped = InstallationSet::new();
    for item in skipped_list {
        skipped.add(&item);
        output.send(Message::SkippedJob(item)).await;
    }

    let jobs: Jobs = (task, todo);
    run_jobs_concurrently(jobs, output, skipped, &helm_repos).await
}

struct HelmRepos {
    url_to_name: HashMap<String, String>,
    next_id: u16,
}

impl HelmRepos {
    fn new() -> HelmRepos {
        HelmRepos {
            url_to_name: HashMap::new(),
            next_id: 0,
        }
    }

    // fn get(&self, url: &str) -> Option<HelmRepo> {
    //     let url = url.to_string();
    //     self.url_to_name.get(&url).map(|name| HelmRepo {
    //         name: name.clone(),
    //         url,
    //     })
    // }

    fn get_or_set(&mut self, url: &str) -> HelmRepo {
        let url = url.to_string();
        let name = if let Some(name) = self.url_to_name.get(&url) {
            name.clone()
        } else {
            let name = format!("helm_{}", self.next_id);
            self.url_to_name.insert(url.clone(), name.clone());
            self.next_id += 1;
            name
        };
        HelmRepo { name, url }
    }

    fn iter(&self) -> HelmIter {
        HelmIter {
            parent: self.url_to_name.iter(),
        }
    }
}

struct HelmIter<'a> {
    parent: Iter<'a, String, String>,
}

impl<'a> Iterator for HelmIter<'a> {
    type Item = HelmRepo;

    fn next(&mut self) -> Option<Self::Item> {
        self.parent.next().map(|(url, name)| HelmRepo {
            name: name.clone(),
            url: url.clone(),
        })
    }
}

type SkippedResult = Arc<Installation>;

#[allow(clippy::cognitive_complexity)]
fn generate_todo(
    args: &Args,
    list_release_names: &[String],
    helm_repos: &mut HelmRepos,
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
                let install = cluster
                    .load_release(release_dir_name, &overrides)
                    .with_context(|| {
                        format!("Cannot load releases from cluster {cluster_name} env {env_name}")
                    })?;
                trace!("Processing install {}", install.name);

                let auto_skip = match args.auto {
                    AutoState::Yes => !install.config.auto,
                    AutoState::All => false,
                };

                // We also do skip entries if the install is to be skipped, these will be shown
                let skip = (!list_release_names.is_empty()
                    && !list_release_names.contains(&install.name))
                    || install.config.locked
                    || auto_skip;

                let installation =
                    create_installation(&env, &cluster, install, next_id, helm_repos);
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
    helm_repos: &mut HelmRepos,
) -> Installation {
    let depends = release.config.depends.unwrap_or_default();

    let mut values_files: Vec<ValuesFile> = vec![];
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

    let encrypted_file = release.dir.join("secrets.sops.yaml");
    if encrypted_file.is_file() {
        values_files.push(ValuesFile {
            path: encrypted_file,
            format: ValuesFormat::Sops,
        });
    }

    let encrypted_file = release.dir.join("secrets.vals.yaml");
    if encrypted_file.is_file() {
        values_files.push(ValuesFile {
            path: encrypted_file,
            format: ValuesFormat::Vals,
        });
    }

    // Turn legacy config into new config
    let release_chart = release.config.release_chart;

    let chart = match release_chart {
        ChartReference::Helm {
            repo_url,
            chart_name,
            chart_version,
        } => {
            let repo = helm_repos.get_or_set(&repo_url);
            HelmChart::HelmRepo {
                repo,
                chart_name,
                chart_version,
            }
        }
        ChartReference::Oci {
            repo_url,
            chart_name,
            chart_version,
        } => HelmChart::OciRepo {
            repo_url,
            chart_name,
            chart_version,
        },
        ChartReference::Local { path } => HelmChart::Dir(path),
    };

    let announce_policy = release
        .config
        .announce_policy
        .unwrap_or(config::AnnouncePolicy::None);

    Installation {
        name: release.name,
        namespace: release.config.namespace,
        env_name: env.name.clone(),
        cluster_name: cluster.name.clone(),
        context: cluster.config.context.clone(),
        values_files,
        chart,
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

fn is_ok(skip_depends: bool, some_i: Option<&Arc<Installation>>, done: &InstallationSet) -> bool {
    some_i.map_or(false, |i| skip_depends || is_depends_ok(i, done))
}

async fn run_jobs_concurrently(
    jobs: Jobs,
    output: &output::MultiOutput,
    skipped: InstallationSet,
    helm_repos: &HelmRepos,
) -> Result<()> {
    let required_repos = get_required_repos(&jobs, helm_repos);
    helm_add_repos(&required_repos).await?;

    let skip_depends = !matches!(jobs.0, Task::Upgrade | Task::Test);
    let task = jobs.0;

    for job in &jobs.1 {
        output.send(Message::NewJob(job.clone())).await;
    }

    // This process receives requests for jobs and dispatches them
    let (tx_dispatch, rx_dispatch) = mpsc::channel(10);
    let dispatch =
        tokio::spawn(
            async move { dispatch_thread(jobs, skipped, rx_dispatch, skip_depends).await },
        );

    // The actual worker threads
    let threads: Vec<JoinHandle<Result<()>>> = (0..NUM_THREADS)
        .map(|_| {
            let tx_dispatch = tx_dispatch.clone();
            let output = output.clone();
            tokio::spawn(async move { worker_thread(task, &tx_dispatch, &output).await })
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

    // When worker threads done we can drop the helm repos
    helm_remove_repos_for_jobs(&required_repos).await?;

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
    jobs: (Task, Vec<Arc<Installation>>),
    skipped: InstallationSet,
    mut rx_dispatch: mpsc::Receiver<Dispatch>,
    skip_depends: bool,
) -> Result<(), anyhow::Error> {
    let mut installations: Vec<Option<&Arc<Installation>>> = jobs.1.iter().map(Some).collect();
    let mut done = skipped;
    while let Some(msg) = rx_dispatch.recv().await {
        match msg {
            Dispatch::RequestNextJob(tx) => {
                // Search for first available installation that meets dependancies.
                let some_i = installations
                    .iter_mut()
                    .find(|i| is_ok(skip_depends, **i, &done))
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
    task: Task,
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
        let result = run_job(task, &install, output).await;
        match &result {
            Ok(()) => {
                // Tell dispatcher job is done so it can update the dependancies
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
