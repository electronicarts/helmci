use std::fs::File;
use std::path::{Path, PathBuf};

use async_compression::futures::write::GzipEncoder;
use futures::AsyncWriteExt;
use serde::Deserialize;
use thiserror::Error;

use crate::repos::cache::Key;
use crate::repos::meta::RepoSpecific;

use super::cache::{self, Cache};
use super::hash::Sha256Hash;
use super::{charts::Chart, meta::Meta};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to download: {0}")]
    Download(#[from] reqwest::Error),
    #[error("Failed to parse index: {0}")]
    Parse(#[from] serde_yml::Error),
    #[error("File error: {0}")]
    File(PathBuf, std::io::Error),
    #[error("Cache error: {0}")]
    Cache(#[from] cache::Error),
    #[error("Invalid JSON")]
    InvalidJson(#[from] serde_json::Error),
    #[error("Size mismatch {0}: expected {1}, but got {2}")]
    SizeMismatch(PathBuf, u64, u64),
}

#[allow(dead_code)]
// BytesMut::freeze uses unsafe but this isn't going to effect serde.
#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    api_version: String,
    app_version: String,
    description: String,
    name: String,
    version: String,
}

impl Entry {
    async fn download_chart(
        &self,
        path: &Path,
        cache: &Cache,
        expected_size: Option<u64>,
        expected_sha256_hash: Option<Sha256Hash>,
    ) -> Result<Chart, Error> {
        if let Some(expected_sha256_hash) = &expected_sha256_hash {
            let key = Key {
                name: self.name.clone(),
                sha256_hash: expected_sha256_hash.clone(),
            };

            if let Some(file_path) = cache.get_cache_entry(&key).await? {
                return self.file_to_chart(file_path, expected_sha256_hash, expected_size);
            }
        }

        let mut temp_file = cache.create_cache_entry(".tar", expected_size).await?;

        {
            let file = temp_file.mut_file();
            let encoder = GzipEncoder::new(file);
            let mut tar = async_tar::Builder::new(encoder);
            tar.append_dir_all(format!("{}-{}", self.name, self.version), path)
                .await
                .map_err(|e| Error::File(path.to_path_buf(), e))?;
            tar.finish()
                .await
                .map_err(|e| Error::File(path.to_path_buf(), e))?;

            let mut encoder = tar
                .into_inner()
                .await
                .map_err(|e| Error::File(path.to_path_buf(), e))?;
            encoder
                .close()
                .await
                .map_err(|e| Error::File(path.to_path_buf(), e))?;
        };

        // we must flush the file before calculating the hash
        temp_file.flush().await?;

        // // If we don't have an expected hash value, just use the real hash
        let sha256_hash = match expected_sha256_hash {
            Some(expected) => expected,
            None => temp_file.calc_sha256_hash().await?,
        };

        let key = Key {
            name: self.name.clone(),
            sha256_hash: sha256_hash.clone(),
        };
        let path = temp_file.finalize(&key).await?;
        self.file_to_chart(path, &sha256_hash, expected_size)
    }

    fn file_to_chart(
        &self,
        file_path: PathBuf,
        sha256_hash: &Sha256Hash,
        expected_size: Option<u64>,
    ) -> Result<Chart, Error> {
        let metadata = file_path
            .metadata()
            .map_err(|e| Error::File(file_path.clone(), e))?;
        let size = metadata.len();

        if let Some(expected_size) = expected_size
            && expected_size != size
        {
            return Err(Error::SizeMismatch(file_path, expected_size, size));
        }

        let meta = Meta {
            name: self.name.clone(),
            version: self.version.clone(),
            sha256_hash: sha256_hash.clone(),
            created: Some(chrono::Utc::now().fixed_offset()),
            repo: RepoSpecific::Helm,
            size,
        };

        let chart = Chart { file_path, meta };

        Ok(chart)
    }
}

pub async fn get_by_path(path: &Path, cache: &Cache) -> Result<Chart, Error> {
    let config_path = path.join("Chart.yaml");
    let file = File::open(config_path.clone()).map_err(|e| Error::File(config_path, e))?;
    let entry: Entry = serde_yml::from_reader(file)?;
    entry.download_chart(path, cache, None, None).await
}

pub async fn get_by_meta(path: &Path, cache: &Cache, meta: &Meta) -> Result<Chart, Error> {
    let config_path = path.join("Chart.yaml");
    let file = File::open(config_path.clone()).map_err(|e| Error::File(config_path, e))?;
    let entry: Entry = serde_yml::from_reader(file)?;
    entry
        .download_chart(path, cache, Some(meta.size), Some(meta.sha256_hash.clone()))
        .await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[tokio::test]
    #[ignore = "requires external file"]
    async fn test_get_by_path() {
        let expected_hash =
            Sha256Hash::new("0a2fa83c8d432594612dde50eadfbd23bb795a0f017815dc8023fb8d9e40d30d");

        let cache = Cache::new(Path::new("tmp/local_get_by_path"));
        let ingress = get_by_path(
            Path::new("/home/brian/tree/ea/cloudcell/helm-charts/charts/ingress"),
            &cache,
        )
        .await
        .unwrap();

        assert_eq!(ingress.meta.name, "ingress");
        assert_eq!(ingress.meta.version, "0.0.1");
        assert_eq!(ingress.meta.sha256_hash, expected_hash);

        assert!(ingress.file_path.exists());

        let hash = Sha256Hash::from_async_path(&ingress.file_path)
            .await
            .unwrap();
        assert_eq!(hash, hash);
    }

    #[tokio::test]
    #[ignore = "requires external file"]
    async fn test_get_by_meta() {
        let expected_hash =
            Sha256Hash::new("0a2fa83c8d432594612dde50eadfbd23bb795a0f017815dc8023fb8d9e40d30d");

        let meta = Meta {
            name: "ingress".to_string(),
            version: "0.0.1".to_string(),
            sha256_hash: expected_hash.clone(),
            created: None,
            repo: RepoSpecific::Helm,
            size: 13312,
        };

        let cache = Cache::new(std::path::Path::new("tmp/local_get_by_meta"));
        let ingress = get_by_meta(
            Path::new("/home/brian/tree/ea/cloudcell/helm-charts/charts/ingress"),
            &cache,
            &meta,
        )
        .await
        .unwrap();
        assert_eq!(ingress.meta.name, "ingress");
        assert_eq!(ingress.meta.version, "0.0.1");
        assert_eq!(ingress.meta.sha256_hash, expected_hash);

        let hash = Sha256Hash::from_async_path(&ingress.file_path)
            .await
            .unwrap();
        assert_eq!(hash, hash);
    }
}
