use serde::{Deserialize, Serialize};

use super::{cache::Key, hash::Sha256Hash};

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Meta {
    pub name: String,
    pub version: String,
    pub sha256_hash: Sha256Hash,
    pub created: Option<chrono::DateTime<chrono::FixedOffset>>,
    pub repo: RepoSpecific,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "type")]
pub enum RepoSpecific {
    Local,
    Oci { manifest_digest: String },
    Helm,
}

impl Meta {
    pub(super) fn get_cache_key(&self) -> Key {
        Key {
            name: self.name.clone(),
            sha256_hash: self.sha256_hash.clone(),
        }
    }
}
