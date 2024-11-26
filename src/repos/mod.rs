pub mod cache;
pub mod charts;
mod hash;
pub mod helm;
pub mod local;
pub mod locks;
pub mod meta;
pub mod oci;

use std::collections::HashMap;

use charts::Chart;
use meta::Meta;
use thiserror::Error;
use url::Url;

use crate::config::ChartReference;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Local repo error retrieving chart {0}: {1}")]
    Local(String, local::Error),
    #[error("Helm repo error retrieving chart {0}: {1}")]
    Helm(ChartReference, helm::Error),
    #[error("Oci repo error retrieving chart {0}: {1}")]
    Oci(ChartReference, oci::Error),
    #[error("Cache error retrieving chart {0}: {1}")]
    Cache(ChartReference, cache::Error),
    #[error("Repo Not Found {0}")]
    RepoNotFound(Url),
    #[error("Version mismatch: requested {0} locked {1}")]
    VersionMismatch(String, String),
    #[error("Name mismatch: requested {0} locked {1}")]
    NameMismatch(String, String),
}

pub async fn download_by_reference(
    repos: &Repos,
    cache: &cache::Cache,
    chart: &ChartReference,
) -> Result<charts::Chart, Error> {
    match chart {
        ChartReference::Local { path } => Ok(local::get_by_path(path, cache)
            .await
            .map_err(|e| Error::Local(path.to_string_lossy().to_string(), e))?),

        ChartReference::Helm {
            repo_url,
            chart_name,
            chart_version,
        } => {
            let repo: &helm::Repo = repos
                .helm
                .get(repo_url)
                .ok_or(Error::RepoNotFound(repo_url.clone()))?;
            let entry = repo
                .get_by_version(chart_name, chart_version, cache)
                .await
                .map_err(|e| Error::Helm(chart.clone(), e))?;
            Ok(entry)
        }

        ChartReference::Oci {
            repo_url,
            chart_name,
            chart_version,
        } => {
            let repo = repos
                .oci
                .get(repo_url)
                .ok_or(Error::RepoNotFound(repo_url.clone()))?;
            let entry = repo
                .get_by_version(chart_name, chart_version, cache)
                .await
                .map_err(|e| Error::Oci(chart.clone(), e))?;
            Ok(entry)
        }
    }
}

pub async fn download_by_meta(
    repos: &Repos,
    cache: &cache::Cache,
    meta: &Meta,
    chart: &ChartReference,
) -> Result<charts::Chart, Error> {
    match chart {
        ChartReference::Local { .. } => {}
        ChartReference::Helm {
            chart_version,
            chart_name,
            ..
        }
        | ChartReference::Oci {
            chart_version,
            chart_name,
            ..
        } => {
            if *chart_version != meta.version {
                return Err(Error::VersionMismatch(
                    chart_version.clone(),
                    meta.version.clone(),
                ));
            }
            if *chart_name != meta.name {
                return Err(Error::NameMismatch(chart_name.clone(), meta.name.clone()));
            }
        }
    }

    let key = meta.get_cache_key();
    if let Some(path) = cache
        .get_cache_entry(&key)
        .await
        .map_err(|e| Error::Cache(chart.clone(), e))?
    {
        let chart = Chart {
            meta: meta.clone(),
            file_path: path,
        };
        return Ok(chart);
    }

    match chart {
        ChartReference::Local { path } => local::get_by_meta(path, cache, meta)
            .await
            .map_err(|e| Error::Local(path.to_string_lossy().to_string(), e)),
        ChartReference::Helm { repo_url, .. } => {
            let repo: &helm::Repo = repos
                .helm
                .get(repo_url)
                .ok_or(Error::RepoNotFound(repo_url.clone()))?;
            repo.get_by_meta(meta, cache)
                .await
                .map_err(|e| Error::Helm(chart.clone(), e))
        }
        ChartReference::Oci { repo_url, .. } => {
            let repo = repos
                .oci
                .get(repo_url)
                .ok_or(Error::RepoNotFound(repo_url.clone()))?;
            repo.get_by_meta(meta, cache)
                .await
                .map_err(|e| Error::Oci(chart.clone(), e))
        }
    }
}

pub struct Repos {
    helm: HashMap<Url, helm::Repo>,
    oci: HashMap<Url, oci::Repo>,
}

impl Repos {
    pub fn new() -> Self {
        Self {
            helm: HashMap::new(),
            oci: HashMap::new(),
        }
    }

    pub async fn download(&mut self, chart: &ChartReference) -> Result<(), Error> {
        match chart {
            ChartReference::Helm { repo_url, .. } => {
                if !self.helm.contains_key(repo_url) {
                    let repo = helm::Repo::download_index(repo_url)
                        .await
                        .map_err(|e| Error::Helm(chart.clone(), e))?;
                    self.helm.insert(repo_url.clone(), repo);
                }
            }
            ChartReference::Oci { repo_url, .. } => {
                if !self.oci.contains_key(repo_url) {
                    let repo = oci::Repo::download_index(repo_url)
                        .map_err(|e| Error::Oci(chart.clone(), e))?;
                    self.oci.insert(repo_url.clone(), repo);
                }
            }
            ChartReference::Local { .. } => {}
        }

        Ok(())
    }
}
