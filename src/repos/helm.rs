use std::collections::HashMap;
use std::path::PathBuf;

use futures::StreamExt;
use thiserror::Error;
use url::Url;

use super::cache::{self, Cache, Key};
use super::hash::Sha256Hash;
use super::meta::RepoSpecific;
use super::{charts::Chart, meta::Meta};
use crate::urls::{AppendUrlError, append_url};
use crate::versions;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to download: {0}")]
    Download(#[from] reqwest::Error),
    #[error("Failed to parse index: {0}")]
    Parse(#[from] serde_yml::Error),
    #[error("Failed to append url: {0}")]
    UrlJoin(#[from] AppendUrlError),
    #[error("File error: {0}")]
    File(PathBuf, std::io::Error),
    #[error("Cache error: {0}")]
    Cache(#[from] cache::Error),
    #[error("Chart not found")]
    ChartNotFound,
    #[error("Failed to parse version: {0}")]
    VersionParse(#[from] versions::Error),
    #[error("Chart missing digest")]
    MissingDigest,
    #[error("Size mismatch {0}: expected {1}, but got {2}")]
    SizeMismatch(PathBuf, u64, u64),
    #[error("Reqwest error {0}: {1}")]
    Reqwest(Url, reqwest::Error),
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    created: chrono::DateTime<chrono::FixedOffset>,
    description: String,
    digest: Option<Sha256Hash>,
    home: Option<String>,
    name: String,
    #[serde(default)]
    sources: Vec<Url>,
    urls: Vec<Url>,
    version: String,
}

impl Entry {
    async fn download_chart(
        &self,
        cache: &Cache,
        expected_size: Option<u64>,
    ) -> Result<Chart, Error> {
        let Some(sha256_hash) = &self.digest else {
            return Err(Error::MissingDigest);
        };

        let key = Key {
            name: self.name.clone(),
            sha256_hash: sha256_hash.clone(),
        };

        if let Some(file) = cache.get_cache_entry(&key).await? {
            self.file_to_chart(file, sha256_hash, expected_size)?;
        }

        let url = self.urls.first().ok_or(Error::ChartNotFound)?;
        let mut file = cache.create_cache_entry(".tgz", expected_size).await?;

        {
            let mut stream = reqwest::get(url.as_str())
                .await?
                .error_for_status()?
                .bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| Error::Reqwest(url.clone(), e))?;
                file.write_chunk(&chunk).await?;
            }
        }

        let path = file.finalize(&key).await?;
        self.file_to_chart(path, sha256_hash, expected_size)
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

        if let Some(expected_size) = expected_size {
            if expected_size != size {
                return Err(Error::SizeMismatch(file_path, expected_size, size));
            }
        }

        let meta = Meta {
            name: self.name.clone(),
            version: self.version.clone(),
            sha256_hash: sha256_hash.clone(),
            created: Some(self.created),
            repo: RepoSpecific::Helm,
            size,
        };

        let chart = Chart { file_path, meta };

        Ok(chart)
    }
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    api_version: String,
    entries: HashMap<String, Vec<Entry>>,
    generated: chrono::DateTime<chrono::FixedOffset>,
}

impl Repo {
    pub async fn download_index(url: &Url) -> Result<Self, Error> {
        let url = append_url(url, "index.yaml")?;
        let response = reqwest::get(url).await?;
        let text = response.text().await?;
        Ok(serde_yml::from_str::<Repo>(&text)?)
    }

    fn internal_get_by_version(&self, name: &str, version: &str) -> Option<&Entry> {
        self.entries
            .get(name)?
            .iter()
            .find(|e| e.version == version)
    }

    fn internal_get_by_sha256(&self, name: &str, expected_hash: &Sha256Hash) -> Option<&Entry> {
        self.entries
            .get(name)?
            .iter()
            .find(|e| e.digest.as_ref() == Some(expected_hash))
    }

    pub async fn get_by_version(
        &self,
        name: &str,
        version: &str,
        cache: &Cache,
    ) -> Result<Chart, Error> {
        let entry = self.internal_get_by_version(name, version);
        if let Some(entry) = entry {
            Ok(entry.download_chart(cache, None).await?)
        } else {
            Err(Error::ChartNotFound)
        }
    }

    pub async fn get_by_meta(&self, meta: &Meta, cache: &Cache) -> Result<Chart, Error> {
        let entry = self.internal_get_by_sha256(&meta.name, &meta.sha256_hash);
        if let Some(entry) = entry {
            Ok(entry.download_chart(cache, Some(meta.size)).await?)
        } else {
            Err(Error::ChartNotFound)
        }
    }

    pub fn get_newest_version(&self, name: &str) -> Result<versions::Version, Error> {
        let list = self.entries.get(name).ok_or(Error::ChartNotFound)?;
        let mut versions = Vec::with_capacity(list.len());

        for entry in list {
            let version = versions::parse_version(&entry.version)?;
            versions.push(version);
        }

        versions.sort();

        match versions.len() {
            0 => Err(Error::ChartNotFound),
            n => Ok(versions.swap_remove(n - 1)),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use chrono::DateTime;

    use super::*;

    #[test]
    fn test_deserialize_index() {
        let index = r"
apiVersion: v1
entries:
  alpine:
    - created: 2016-10-06T16:23:20.499814565-06:00
      description: Deploy a basic Alpine Linux pod
      digest: 99c76e403d752c84ead610644d4b1c2f2b453a74b921f422b9dcb8a7c8b559cd
      home: https://helm.sh/helm
      name: alpine
      sources:
      - https://github.com/helm/helm
      urls:
      - https://technosophos.github.io/tscharts/alpine-0.2.0.tgz
      version: 0.2.0
    - created: 2016-10-06T16:23:20.499543808-06:00
      description: Deploy a basic Alpine Linux pod
      digest: 515c58e5f79d8b2913a10cb400ebb6fa9c77fe813287afbacf1a0b897cd78727
      home: https://helm.sh/helm
      name: alpine
      sources:
      - https://github.com/helm/helm
      urls:
      - https://technosophos.github.io/tscharts/alpine-0.1.0.tgz
      version: 0.1.0
  nginx:
    - created: 2016-10-06T16:23:20.499543808-06:00
      description: Create a basic nginx HTTP server
      digest: aaff4545f79d8b2913a10cb400ebb6fa9c77fe813287afbacf1a0b897cdffffff
      home: https://helm.sh/helm
      name: nginx
      sources:
      - https://github.com/helm/charts
      urls:
      - https://technosophos.github.io/tscharts/nginx-1.1.0.tgz
      version: 1.1.0
generated: 2016-10-06T16:23:20.499029981-06:00
";
        let text = serde_yml::from_str::<Repo>(index).unwrap();
        assert_eq!(text.api_version, "v1");
        assert_eq!(text.entries.len(), 2);
        assert_eq!(
            text.generated,
            DateTime::parse_from_rfc3339("2016-10-06T16:23:20.499029981-06:00").unwrap()
        );
    }

    #[tokio::test]
    #[ignore]
    async fn test_download_by_version() {
        let index = Repo::download_index(
            &Url::parse("https://kubernetes.github.io/ingress-nginx/").unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(index.api_version, "v1");
        // assert_eq!(index.entries.len(), 2);

        let expected_hash =
            Sha256Hash::new("a9f371489a6c18042508c0b5c2ea669deaad7ccc08f37efc4522e336edda1c20");

        let cache = Cache::new(std::path::Path::new("tmp/helm_download_by_version"));
        let ingress = index
            .get_by_version("ingress-nginx", "4.11.3", &cache)
            .await
            .unwrap();
        assert_eq!(ingress.meta.name, "ingress-nginx");
        assert_eq!(ingress.meta.version, "4.11.3");
        assert_eq!(ingress.meta.sha256_hash, expected_hash);

        assert!(ingress.file_path.exists());

        let hash = Sha256Hash::from_async_path(&ingress.file_path)
            .await
            .unwrap();
        assert_eq!(hash, hash);
    }

    #[tokio::test]
    #[ignore]
    async fn test_download_by_meta() {
        let index = Repo::download_index(
            &Url::parse("https://kubernetes.github.io/ingress-nginx/").unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(index.api_version, "v1");
        // assert_eq!(index.entries.len(), 2);

        let expected_hash =
            Sha256Hash::new("a9f371489a6c18042508c0b5c2ea669deaad7ccc08f37efc4522e336edda1c20");

        let meta = Meta {
            name: "ingress-nginx".to_string(),
            version: "4.11.3".to_string(),
            sha256_hash: expected_hash.clone(),
            created: None,
            repo: RepoSpecific::Helm,
            size: 58282,
        };

        let cache = Cache::new(std::path::Path::new("tmp/helm_download_by_meta"));
        let ingress = index.get_by_meta(&meta, &cache).await.unwrap();
        assert_eq!(ingress.meta.name, "ingress-nginx");
        assert_eq!(ingress.meta.version, "4.11.3");
        assert_eq!(ingress.meta.sha256_hash, expected_hash);

        let hash = Sha256Hash::from_async_path(&ingress.file_path)
            .await
            .unwrap();
        assert_eq!(hash, hash);
    }
}
