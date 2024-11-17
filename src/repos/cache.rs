use std::fmt::Debug;
use std::path::Path;
use std::path::PathBuf;

use bytes::Bytes;
use futures::Stream;
use futures::StreamExt;
use thiserror::Error;
use tokio::io::AsyncWriteExt;

use super::charts::Chart;
use super::{hash::Sha256Hash, meta::Meta};

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("Digest mismatch {0}: {1} != {2}")]
    DigestMismatch(PathBuf, Sha256Hash, Sha256Hash),
    // #[error("Invalid JSON {0}: {1}")]
    // InvalidJson(PathBuf, serde_json::Error),
}

#[derive(Error, Debug)]
pub enum StreamError<E> {
    #[error("IO error {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("Digest mismatch {0}: {1} != {2}")]
    DigestMismatch(PathBuf, Sha256Hash, Sha256Hash),
    // #[error("Invalid JSON {0}: {1}")]
    // InvalidJson(PathBuf, serde_json::Error),
    #[error("Stream error: {0}")]
    Stream(PathBuf, E),
}

#[derive(Debug)]
pub struct Cache {
    pub path: std::path::PathBuf,
}

impl Cache {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.join(".charts"),
        }
    }

    fn get_cache_file_path(&self, chart_name: &str, digest: &Sha256Hash) -> PathBuf {
        self.path.join(chart_name).join(format!("{digest}.tgz"))
    }

    fn get_cache_temp_path(&self, chart_name: &str, digest: &Sha256Hash) -> PathBuf {
        self.path.join(chart_name).join(format!("{digest}.tmp"))
    }

    pub(super) async fn get_cache_entry(&self, meta: &Meta) -> Result<Option<Chart>, Error> {
        let file_path = self.get_cache_file_path(&meta.name, &meta.sha256_hash);

        if !file_path.exists() {
            return Ok(None);
        }

        let sha256_hash = Sha256Hash::from_file_async(&file_path)
            .await
            .map_err(|e| Error::Io(file_path.clone(), e))?;

        if meta.sha256_hash != sha256_hash {
            return Err(Error::DigestMismatch(
                file_path.clone(),
                meta.sha256_hash.clone(),
                sha256_hash,
            ));
        }

        Ok(Some(Chart {
            file_path,
            meta: meta.clone(),
        }))
    }

    pub(super) async fn create_cache_entry<E: Send>(
        &self,
        meta: Meta,
        mut stream: impl Stream<Item = Result<Bytes, E>> + Unpin + Send,
    ) -> Result<Chart, StreamError<E>> {
        let file_path = self.get_cache_file_path(&meta.name, &meta.sha256_hash);
        let temp_path = self.get_cache_temp_path(&meta.name, &meta.sha256_hash);

        if let Some(parent) = temp_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StreamError::Io(temp_path.clone(), e))?;
        }

        {
            let mut file = tokio::fs::File::create(&temp_path)
                .await
                .map_err(|e| StreamError::Io(temp_path.clone(), e))?;

            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| StreamError::Stream(temp_path.clone(), e))?;

                file.write_all(&chunk)
                    .await
                    .map_err(|e| StreamError::Io(temp_path.clone(), e))?;
            }
        }

        let sha256_hash = Sha256Hash::from_file_async(&temp_path)
            .await
            .map_err(|e| StreamError::Io(temp_path.clone(), e))?;
        if meta.sha256_hash != sha256_hash {
            return Err(StreamError::DigestMismatch(
                temp_path,
                meta.sha256_hash.clone(),
                sha256_hash,
            ));
        }

        tokio::fs::rename(&temp_path, &file_path)
            .await
            .map_err(|e| StreamError::Io(file_path.clone(), e))?;

        Ok(Chart { file_path, meta })
    }
}
