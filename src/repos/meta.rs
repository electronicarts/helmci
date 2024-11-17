use serde::{Deserialize, Serialize};

use super::hash::Sha256Hash;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Meta {
    pub name: String,
    pub version: String,
    pub sha256_hash: Sha256Hash,
    pub created: Option<chrono::DateTime<chrono::FixedOffset>>,
    pub repo: RepoSpecific,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "type")]
pub enum RepoSpecific {
    Local,
    Oci { digest: String },
    Helm,
}
