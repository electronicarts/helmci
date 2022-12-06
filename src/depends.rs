// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.

//! Work out if dependancies for installation have been satisified or not.
use std::{collections::HashSet, hash::Hash};

use crate::{config::ReleaseReference, helm::Installation};

/// An unique identifier for a particular installation.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct HashIndex(
    /// Kubernetes context.
    String,
    /// Kubernetes namespace.
    String,
    /// Kubernetes release.
    String,
);

impl HashIndex {
    /// Create a new `HashIndex`.
    pub fn get_hash_index(i: &Installation) -> Self {
        HashIndex(i.context.clone(), i.namespace.clone(), i.name.clone())
    }

    /// Change the name of an index and return new object.
    pub fn new_name(&self, r: &ReleaseReference) -> Self {
        HashIndex(self.0.clone(), r.namespace.clone(), r.name.clone())
    }
}

/// A set containing a number of installation references.
#[derive(Default)]
pub struct InstallationSet(HashSet<HashIndex>);

impl InstallationSet {
    /// Create a new `InstallationSet`.
    #[allow(dead_code)]
    pub fn new() -> Self {
        InstallationSet(HashSet::new())
    }

    /// Add an installation to the set.
    pub fn add(&mut self, i: &Installation) {
        self.0.insert(HashIndex::get_hash_index(i));
    }

    /// Add a supplied hash to the set.
    pub fn add_hash(&mut self, i: HashIndex) {
        self.0.insert(i);
    }

    /// Does this set contain an installation?
    pub fn contains(&self, i: &Installation) -> bool {
        self.0.contains(&HashIndex::get_hash_index(i))
    }
}

/// Has the depends been satisified for this installation?
pub fn is_depends_ok(installation: &Installation, done: &InstallationSet) -> bool {
    let index = HashIndex::get_hash_index(installation);

    installation
        .depends
        .iter()
        .all(|value| done.0.contains(&index.new_name(value)))
        || installation.depends.is_empty()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::config::AnnouncePolicy;

    #[allow(clippy::wildcard_imports)]
    use crate::{
        config::ReleaseReference,
        depends::*,
        helm::{HelmChart, HelmRepo, Installation},
    };

    #[test]
    fn test_get_depends_order_good() {
        let todo = vec![
            Installation {
                name: "ingress-nginx-ext".to_string(),
                values_files: vec![],
                env_name: "dev".to_string(),
                cluster_name: "spartan".to_string(),
                chart: HelmChart::HelmRepo {
                    repo: HelmRepo {
                        name: "ingress-nginx".to_string(),
                        url: "https://kubernetes.github.io/ingress-nginx".to_string(),
                    },
                    chart_name: "ingress-nginx".to_string(),
                    chart_version: "4.0.10".to_string(),
                },
                depends: vec![],
                context: "my-cluster".to_string(),
                namespace: "kube-system".to_string(),
                timeout: 180,
                id: 0,
                announce_policy: AnnouncePolicy::None,
            },
            Installation {
                name: "ingress-nginx-ext-2".to_string(),
                values_files: vec![],
                env_name: "dev".to_string(),
                cluster_name: "spartan".to_string(),
                chart: HelmChart::HelmRepo {
                    repo: HelmRepo {
                        name: "ingress-nginx".to_string(),
                        url: "https://kubernetes.github.io/ingress-nginx".to_string(),
                    },
                    chart_name: "ingress-nginx".to_string(),
                    chart_version: "4.0.10".to_string(),
                },
                depends: vec![],
                context: "my-cluster".to_string(),
                namespace: "kube-system".to_string(),
                timeout: 180,
                id: 1,
                announce_policy: AnnouncePolicy::None,
            },
            Installation {
                name: "exporter-blackbox".to_string(),
                values_files: vec![],
                env_name: "dev".to_string(),
                cluster_name: "spartan".to_string(),
                chart: HelmChart::Dir(PathBuf::from("charts/prometheus-blackbox-exporter")),
                depends: vec![ReleaseReference {
                    namespace: "kube-system".to_string(),
                    name: "ingress-nginx-ext".to_string(),
                }],
                context: "my-cluster".to_string(),
                namespace: "monitoring".to_string(),
                timeout: 180,
                id: 2,
                announce_policy: AnnouncePolicy::None,
            },
            Installation {
                name: "exporter-blackbox-2".to_string(),
                values_files: vec![],
                env_name: "dev".to_string(),
                cluster_name: "spartan".to_string(),
                chart: HelmChart::Dir(PathBuf::from("charts/prometheus-blackbox-exporter")),
                depends: vec![ReleaseReference {
                    namespace: "kube-system".to_string(),
                    name: "ingress-nginx-ext".to_string(),
                }],
                context: "my-cluster".to_string(),
                namespace: "monitoring".to_string(),
                timeout: 180,
                id: 3,
                announce_policy: AnnouncePolicy::None,
            },
        ];

        let mut done = InstallationSet::new();
        assert!(is_depends_ok(&todo[0], &done));
        assert!(is_depends_ok(&todo[1], &done));
        assert!(!is_depends_ok(&todo[2], &done));
        assert!(!is_depends_ok(&todo[3], &done));

        done.add(&todo[0]);
        assert!(is_depends_ok(&todo[0], &done));
        assert!(is_depends_ok(&todo[1], &done));
        assert!(is_depends_ok(&todo[2], &done));
        assert!(is_depends_ok(&todo[3], &done));
    }

    #[test]
    fn test_get_depends_order_multiple_depends() {
        let todo = vec![
            Installation {
                name: "ingress-nginx-ext".to_string(),
                values_files: vec![],
                env_name: "dev".to_string(),
                cluster_name: "spartan".to_string(),
                chart: HelmChart::HelmRepo {
                    repo: HelmRepo {
                        name: "ingress-nginx".to_string(),
                        url: "https://kubernetes.github.io/ingress-nginx".to_string(),
                    },
                    chart_name: "ingress-nginx".to_string(),
                    chart_version: "4.0.10".to_string(),
                },
                depends: vec![],
                context: "my-cluster".to_string(),
                namespace: "kube-system".to_string(),
                timeout: 180,
                id: 0,
                announce_policy: AnnouncePolicy::None,
            },
            Installation {
                name: "exporter-blackbox".to_string(),
                values_files: vec![],
                env_name: "dev".to_string(),
                cluster_name: "spartan".to_string(),
                chart: HelmChart::Dir(PathBuf::from("charts/prometheus-blackbox-exporter")),
                depends: vec![
                    ReleaseReference {
                        namespace: "kube-system".to_string(),
                        name: "ingress-nginx-ext".to_string(),
                    },
                    ReleaseReference {
                        namespace: "kube-system".to_string(),
                        name: "ingress-nginx-ext-bad".to_string(),
                    },
                ],
                context: "my-cluster".to_string(),
                namespace: "monitoring".to_string(),
                timeout: 180,
                id: 1,
                announce_policy: AnnouncePolicy::None,
            },
        ];

        let mut done = InstallationSet::new();
        assert!(is_depends_ok(&todo[0], &done));
        assert!(!is_depends_ok(&todo[1], &done));

        done.add(&todo[0]);
        assert!(is_depends_ok(&todo[0], &done));
        assert!(!is_depends_ok(&todo[1], &done));

        done.add(&todo[1]);
        assert!(is_depends_ok(&todo[0], &done));
        assert!(!is_depends_ok(&todo[1], &done));
    }

    #[test]
    fn test_get_depends_order_loop() {
        // test dependancy loop
        let todo = vec![
            Installation {
                name: "ingress-nginx-ext".to_string(),
                values_files: vec![],
                env_name: "dev".to_string(),
                cluster_name: "spartan".to_string(),
                chart: HelmChart::HelmRepo {
                    repo: HelmRepo {
                        name: "ingress-nginx".to_string(),
                        url: "https://kubernetes.github.io/ingress-nginx".to_string(),
                    },
                    chart_name: "ingress-nginx".to_string(),
                    chart_version: "4.0.10".to_string(),
                },
                depends: vec![ReleaseReference {
                    namespace: "monitoring".to_string(),
                    name: "exporter-blackbox".to_string(),
                }],
                context: "my-cluster".to_string(),
                namespace: "kube-system".to_string(),
                timeout: 180,
                id: 0,
                announce_policy: AnnouncePolicy::None,
            },
            Installation {
                name: "exporter-blackbox".to_string(),
                values_files: vec![],
                env_name: "dev".to_string(),
                cluster_name: "spartan".to_string(),
                chart: HelmChart::Dir(PathBuf::from("charts/prometheus-blackbox-exporter")),
                depends: vec![ReleaseReference {
                    namespace: "kube-system".to_string(),
                    name: "ingress-nginx-ext".to_string(),
                }],
                context: "my-cluster".to_string(),
                namespace: "monitoring".to_string(),
                timeout: 180,
                id: 1,
                announce_policy: AnnouncePolicy::None,
            },
        ];

        let mut done = InstallationSet::new();
        assert!(!is_depends_ok(&todo[0], &done));
        assert!(!is_depends_ok(&todo[1], &done));

        done.add(&todo[0]);
        assert!(!is_depends_ok(&todo[0], &done));
        assert!(is_depends_ok(&todo[1], &done));

        done.add(&todo[1]);
        assert!(is_depends_ok(&todo[0], &done));
        assert!(is_depends_ok(&todo[1], &done));
    }

    #[test]
    fn test_get_depends_itself() {
        // test item depends on itself
        let todo = vec![Installation {
            name: "ingress-nginx-ext".to_string(),
            values_files: vec![],
            env_name: "dev".to_string(),
            cluster_name: "spartan".to_string(),
            chart: HelmChart::HelmRepo {
                repo: HelmRepo {
                    name: "ingress-nginx".to_string(),
                    url: "https://kubernetes.github.io/ingress-nginx".to_string(),
                },
                chart_name: "ingress-nginx".to_string(),
                chart_version: "4.0.10".to_string(),
            },
            depends: vec![ReleaseReference {
                namespace: "kube-system".to_string(),
                name: "ingress-nginx-ext".to_string(),
            }],
            context: "my-cluster".to_string(),
            namespace: "kube-system".to_string(),
            timeout: 180,
            id: 0,
            announce_policy: AnnouncePolicy::None,
        }];

        let mut done = InstallationSet::new();
        assert!(!is_depends_ok(&todo[0], &done));

        done.add(&todo[0]);
        assert!(is_depends_ok(&todo[0], &done));
    }

    #[test]
    fn test_get_depends_non_existant() {
        // test item depends on non-existant install
        let todo = vec![Installation {
            name: "ingress-nginx-ext".to_string(),
            values_files: vec![],
            env_name: "dev".to_string(),
            cluster_name: "spartan".to_string(),
            chart: HelmChart::HelmRepo {
                repo: HelmRepo {
                    name: "ingress-nginx".to_string(),
                    url: "https://kubernetes.github.io/ingress-nginx".to_string(),
                },
                chart_name: "ingress-nginx".to_string(),
                chart_version: "4.0.10".to_string(),
            },
            depends: vec![ReleaseReference {
                namespace: "kube-system".to_string(),
                name: "ingress-nginx-ext-xxx".to_string(),
            }],
            context: "my-cluster".to_string(),
            namespace: "kube-system".to_string(),
            timeout: 180,
            id: 0,
            announce_policy: AnnouncePolicy::None,
        }];

        let mut done = InstallationSet::new();
        assert!(!is_depends_ok(&todo[0], &done));

        done.add(&todo[0]);
        assert!(!is_depends_ok(&todo[0], &done));
    }
}
