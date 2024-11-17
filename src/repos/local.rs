use std::fs::File;
use std::path::Path;

use bytes::BytesMut;
use flate2::read::GzEncoder;
use flate2::Compression;
use futures::stream::TryStreamExt;
use futures_util::TryFutureExt;
use serde::Deserialize;
use thiserror::Error;
use tokio::pin;
use tokio_util::codec::{BytesCodec, FramedRead};

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
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Cache error: {0}")]
    Cache(#[from] cache::Error),
    #[error("Cache streaming error: {0}")]
    CacheStream(#[from] cache::StreamError<std::io::Error>),
    #[error("Invalid JSON")]
    InvalidJson(#[from] serde_json::Error),
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
    async fn download_chart(&self, path: &Path, cache: &Cache) -> Result<Chart, Error> {
        // FIXME: management of temp file is yuck
        let tempfile = Path::new("archive.tar.gz");
        {
            let tar_gz = File::create(tempfile)?;
            let enc = GzEncoder::new(tar_gz, Compression::default());
            let mut tar = tar::Builder::new(enc);
            tar.append_dir_all(format!("{}-{}", self.name, self.version), path)?;
        }

        let sha256_hash = Sha256Hash::from_file_async(tempfile).await?;

        let chart = Meta {
            name: self.name.clone(),
            version: self.version.clone(),
            sha256_hash,
            created: Some(chrono::Utc::now().fixed_offset()),
            repo: RepoSpecific::Local,
        };

        if let Some(entry) = cache.get_cache_entry(&chart).await? {
            return Ok(entry);
        }

        let stream = tokio::fs::File::open(tempfile)
            .map_ok(|file| FramedRead::new(file, BytesCodec::new()).map_ok(BytesMut::freeze))
            .try_flatten_stream();

        pin!(stream);

        let cache = cache.create_cache_entry(chart, stream).await?;
        Ok(cache)
    }
}

pub async fn get_by_path(path: &Path, cache: &Cache) -> Result<Chart, Error> {
    let config_path = path.join("Chart.yaml");
    let entry: Entry = serde_yml::from_reader(File::open(config_path)?)?;
    entry.download_chart(path, cache).await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[tokio::test]
    #[ignore]
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

        let hash = Sha256Hash::from_file_async(&ingress.file_path)
            .await
            .unwrap();
        assert_eq!(hash, hash);
    }
}
