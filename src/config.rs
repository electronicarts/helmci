// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.

//! Read config files from helm-values repo.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use serde::de::Error;
use serde::{Deserialize, Serialize, Serializer};
use url::Url;

use crate::utils::filename_to_string;

#[derive(Deserialize, Debug)]
pub enum ValuesFormat {
    PlainText,
    Vals,
    Sops,
}

#[derive(Deserialize, Debug)]
pub struct ValuesFile {
    pub path: PathBuf,
    pub format: ValuesFormat,
}

/// A Env config file.
///
/// Retrieved from `./envs/$env/config.yaml`.
#[derive(Serialize, Deserialize, Debug)]
pub struct EnvConfig {
    pub locked: bool,
}

/// A cluster config file.
///
/// Retrieved from `./envs/$env/$cluster/config.yaml`.
#[derive(Serialize, Deserialize, Debug)]
pub struct ClusterConfig {
    pub locked: bool,
    pub context: String,
}

/// A reference to another release stored in the config file.
#[derive(Clone, Debug)]
pub struct ReleaseReference {
    pub namespace: String,
    pub name: String,
}

impl<'a> Deserialize<'a> for ReleaseReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;

        match s.split_once('/') {
            Some((namespace, name)) => Ok(ReleaseReference {
                namespace: namespace.to_string(),
                name: name.to_string(),
            }),
            None => Err(Error::custom(format!("Invalid reference {s}"))),
        }
    }
}

impl Serialize for ReleaseReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let str = format!("{}/{}", self.namespace, self.name);
        serializer.serialize_str(&str)
    }
}

/// A reference to a chart stored in the config file.
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
#[serde(tag = "type")]
pub enum ChartReference {
    #[serde(rename = "helm")]
    Helm {
        repo_url: Url,
        chart_name: String,
        chart_version: String,
    },
    #[serde(rename = "oci")]
    Oci {
        repo_url: Url,
        chart_name: String,
        chart_version: String,
    },
    #[serde(rename = "local")]
    Local { path: PathBuf },
}

impl std::fmt::Display for ChartReference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChartReference::Helm {
                repo_url,
                chart_name,
                chart_version,
            } => write!(f, "Helm: {repo_url}/{chart_name}:{chart_version}",),
            ChartReference::Oci {
                repo_url,
                chart_name,
                chart_version,
            } => write!(f, "Oci: {repo_url}/{chart_name}:{chart_version}",),
            ChartReference::Local { path } => write!(f, "Local: {path}", path = path.display()),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AnnouncePolicy {
    UpgradeOnly,
    AllTasks,
    None,
}

/// A release config file.
///
/// Retrieved from `./envs/$env/$cluster/$release_dir/config.yaml`.
#[derive(Deserialize, Debug)]
pub struct ReleaseConfig {
    pub auto: bool,
    pub locked: bool,
    pub namespace: String,
    pub timeout: u16,
    pub release: String,
    pub release_chart: ChartReference,
    pub depends: Option<Vec<ReleaseReference>>,
    pub announce_policy: Option<AnnouncePolicy>,

    /// These files are processed first have have lowest priority.
    #[serde(default)]
    pub base_values_files: Vec<ValuesFile>,

    /// These files are processed last and have highest priority.
    #[serde(default)]
    pub override_values_files: Vec<ValuesFile>,
}

/// Parsed details concerning an Env.
pub struct Env {
    pub name: String,
    pub dir: PathBuf,
    pub config: EnvConfig,
}

/// Parsed details concerning a cluster.
pub struct Cluster {
    pub name: String,
    pub dir: PathBuf,
    pub config: ClusterConfig,
}

///Parsed details concerning a release.
pub struct Release {
    pub name: String,
    pub dir: PathBuf,
    pub config_file: PathBuf,
    pub lock_file: PathBuf,
    pub config: ReleaseConfig,
}

impl Env {
    /// Load the Env given its name.
    pub fn load(vdir: &Path, name: &str) -> Result<Self> {
        let all_envs_dir = vdir.join("envs").join(name);
        let config_file = all_envs_dir.join("config.yaml");

        let file: String = std::fs::read_to_string(&config_file)
            .with_context(|| format!("Reading file {}", config_file.display()))?;
        let config: EnvConfig = serde_yml::from_str(&file)
            .with_context(|| format!("Parsing file {}", config_file.display()))?;

        Ok(Env {
            name: name.to_string(),
            dir: all_envs_dir,
            config,
        })
    }

    /// List all supplied envs.
    pub fn list_all_env(vdir: &Path) -> Result<Vec<String>> {
        let all_envs_dir = vdir.join("envs");
        let mut result = vec![];

        for entry in std::fs::read_dir(all_envs_dir)? {
            let env_dir = entry?.path();

            let name = filename_to_string(&env_dir)?;

            if env_dir.is_dir() {
                result.push(name);
            }
        }

        Ok(result)
    }

    /// For an env list all clusters supplied.
    pub fn list_all_clusters(&self) -> Result<Vec<String>> {
        let dir = &self.dir;
        let mut result = vec![];

        for entry in std::fs::read_dir(dir)? {
            let dir = entry?.path();

            let name = filename_to_string(&dir)?;

            if dir.is_dir() {
                result.push(name);
            }
        }

        Ok(result)
    }

    /// Load a cluster given its name.
    pub fn load_cluster(&self, name: &str) -> Result<Cluster> {
        let dir = self.dir.join(name);
        let config_file = dir.join("config.yaml");

        let file: String = std::fs::read_to_string(&config_file)
            .with_context(|| format!("Reading file {}", config_file.display()))?;
        let config: ClusterConfig = serde_yml::from_str(&file)
            .with_context(|| format!("Parsing file {}", config_file.display()))?;

        Ok(Cluster {
            name: name.to_string(),
            dir,
            config,
        })
    }
}

impl Cluster {
    /// List all releases for a cluster.
    pub fn list_all_releases(&self) -> Result<Vec<String>> {
        let dir = &self.dir;
        let mut result = vec![];

        for entry in std::fs::read_dir(dir)? {
            let dir = entry?.path();

            let name = filename_to_string(&dir)?;

            if dir.is_dir() {
                result.push(name);
            }
        }

        Ok(result)
    }

    /// Load a release given its name and overrides.
    pub fn load_release(&self, dir_name: &str) -> Result<Release> {
        let dir = self.dir.join(dir_name);
        let config_file = dir.join("config.yaml");
        let lock_file = dir.join("lock.json");

        let file: String = std::fs::read_to_string(&config_file)
            .with_context(|| format!("Reading file {}", config_file.display()))?;
        let config: ReleaseConfig = serde_yml::from_str(&file)
            .with_context(|| format!("Parsing file {}", config_file.display()))?;

        Ok(Release {
            name: config.release.clone(),
            dir,
            config_file,
            lock_file,
            config,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ReleaseReference;

    #[test]
    fn deserialize_release_reference() {
        #![allow(clippy::unwrap_used)]

        let str = "'Hello/World'".to_string();
        let result: ReleaseReference = serde_yml::from_str(&str).unwrap();
        assert_eq!(result.namespace, "Hello");
        assert_eq!(result.name, "World");
    }

    #[test]
    fn serialize_release_reference() {
        #![allow(clippy::unwrap_used)]

        let r = ReleaseReference {
            namespace: "Hello".to_string(),
            name: "World".to_string(),
        };
        let result = serde_yml::to_string(&r).unwrap();
        assert_eq!(result, "Hello/World\n");
    }
}
