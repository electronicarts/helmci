use serde::{Deserialize, Serialize};
use sha256::TrySha256Digest;

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone)]
pub struct Sha256Hash(String);

impl Sha256Hash {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    // pub fn from_file_blocking(path: &std::path::Path) -> Result<Self, std::io::Error> {
    //     let bytes = std::fs::read(path)?;
    //     let hash = sha256::digest(&bytes);
    //     Ok(Self(hash))
    // }

    pub async fn from_async_path(path: &std::path::Path) -> Result<Self, std::io::Error> {
        let hash = path.async_digest().await?;
        Ok(Self(hash))
    }

    // pub fn from_bytes(bytes: &[u8]) -> Self {
    //     let hash = sha256::digest(bytes);
    //     Self(hash)
    // }
}

impl std::fmt::Display for Sha256Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
