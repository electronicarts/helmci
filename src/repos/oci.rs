use std::path::PathBuf;

use docker_credential::{CredentialRetrievalError, DockerCredential};
use futures::StreamExt;
use oci_client::{Client, Reference, manifest::OciManifest, secrets::RegistryAuth};
use thiserror::Error;
use url::Url;

use crate::repos::{hash::Sha256Hash, meta::RepoSpecific};

use super::{
    cache::{self, Cache, Key},
    charts::Chart,
    meta::Meta,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("File error: {0}")]
    File(PathBuf, std::io::Error),
    #[error("OCI IO error: {0}")]
    OciIo(Box<Reference>, std::io::Error),
    #[error("Cache error: {0}")]
    Cache(#[from] cache::Error),
    #[error("Invalid OCI URL")]
    InvalidOciUrl(Url),
    #[error("OCI error: {0}")]
    OciDistribution(Box<Reference>, oci_client::errors::OciDistributionError),
    #[error("Not an OCI image")]
    NotAnOciImage,
    #[error("Invalid JSON")]
    InvalidJson(#[from] serde_json::Error),
    #[error("Invalid date: {0}")]
    InvalidDate(#[from] chrono::ParseError),
    #[error("Failed to retrieve docker credentials: {0}")]
    CredentialRetrieval(#[from] CredentialRetrievalError),
    #[error("Unsupported docker credentials")]
    UnsupportedDockerCredentials,
    #[error("Size mismatch {0}: expected {1}, but got {2}")]
    SizeMismatch(PathBuf, u64, u64),
}

#[allow(dead_code)]
pub struct Repo {
    host: String,
    path: String,
}

impl Repo {
    pub fn download_index(url: &Url) -> Result<Self, Error> {
        if url.scheme() != "oci" {
            return Err(Error::InvalidOciUrl(url.clone()));
        }

        let host = url
            .host_str()
            .ok_or(Error::InvalidOciUrl(url.clone()))?
            .to_string();
        let path = String::from(url.path());

        Ok(Repo { host, path })
    }

    async fn get_by_reference(
        &self,
        reference: &Reference,
        cache: &Cache,
        name: &str,
        version: &str,
        expected_size: Option<u64>,
    ) -> Result<Chart, Error> {
        let auth = build_auth(reference)?;
        let client_config = build_client_config();
        let client = Client::new(client_config);

        let (manifest, manifest_digest) = client
            .pull_manifest(reference, &auth)
            .await
            .map_err(|e| Error::OciDistribution(Box::new(reference.clone()), e))?;

        let OciManifest::Image(manifest) = manifest else {
            return Err(Error::NotAnOciImage);
        };

        let [layer] = manifest.layers.as_slice() else {
            return Err(Error::NotAnOciImage);
        };

        let Some(hash) = layer.digest.strip_prefix("sha256:") else {
            return Err(Error::NotAnOciImage);
        };
        let sha256_hash = Sha256Hash::new(hash);

        let annotations = manifest.annotations;

        let created = annotations
            .as_ref()
            .and_then(|a| a.get("org.opencontainers.image.created"))
            .and_then(|date| date.parse().ok());

        let key = Key {
            name: name.to_string(),
            sha256_hash: sha256_hash.clone(),
        };

        if let Some(file_path) = cache.get_cache_entry(&key).await? {
            return file_to_chart(
                file_path,
                name,
                version,
                created,
                sha256_hash,
                expected_size,
                manifest_digest,
            );
        }

        let mut file = cache.create_cache_entry(".tgz", expected_size).await?;

        {
            let mut stream = client
                .pull_blob_stream(reference, &layer)
                .await
                .map_err(|e| Error::OciDistribution(Box::new(reference.clone()), e))?;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| Error::OciIo(Box::new(reference.clone()), e))?;
                file.write_chunk(&chunk).await?;
            }
        }

        let path = file.finalize(&key).await?;
        file_to_chart(
            path,
            name,
            version,
            created,
            sha256_hash,
            expected_size,
            manifest_digest,
        )
    }

    pub async fn get_by_version(
        &self,
        name: &str,
        version: &str,
        cache: &Cache,
    ) -> Result<Chart, Error> {
        let path = format!("{}/{}", self.path, name);
        let reference: Reference =
            Reference::with_tag(self.host.clone(), path, version.to_string());
        self.get_by_reference(&reference, cache, name, version, None)
            .await
    }

    pub async fn get_by_meta(&self, meta: &Meta, cache: &Cache) -> Result<Chart, Error> {
        let path = format!("{}/{}", self.path, meta.name);
        let RepoSpecific::Oci {
            manifest_digest: digest,
        } = &meta.repo
        else {
            return Err(Error::NotAnOciImage);
        };

        let reference: Reference =
            Reference::with_digest(self.host.clone(), path, digest.to_string());
        self.get_by_reference(
            &reference,
            cache,
            &meta.name,
            &meta.version,
            Some(meta.size),
        )
        .await
    }
}

fn file_to_chart(
    file_path: PathBuf,
    name: &str,
    version: &str,
    created: Option<chrono::DateTime<chrono::FixedOffset>>,
    sha256_hash: Sha256Hash,
    expected_size: Option<u64>,
    manifest_digest: String,
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
        name: name.to_string(),
        version: version.to_string(),
        sha256_hash,
        created,
        repo: RepoSpecific::Oci { manifest_digest },
        size,
    };

    let chart = Chart { file_path, meta };

    Ok(chart)
}

fn build_auth(reference: &Reference) -> Result<RegistryAuth, Error> {
    let server = reference
        .resolve_registry()
        .strip_suffix('/')
        .unwrap_or_else(|| reference.resolve_registry());

    #[allow(clippy::match_same_arms)]
    match docker_credential::get_credential(server) {
        Err(CredentialRetrievalError::ConfigNotFound) => Ok(RegistryAuth::Anonymous),
        Err(CredentialRetrievalError::NoCredentialConfigured) => Ok(RegistryAuth::Anonymous),
        Err(err) => Err(err.into()),
        Ok(DockerCredential::UsernamePassword(username, password)) => {
            Ok(RegistryAuth::Basic(username, password))
        }
        Ok(DockerCredential::IdentityToken(_)) => Err(Error::UnsupportedDockerCredentials),
    }
}

fn build_client_config() -> oci_client::client::ClientConfig {
    let protocol = oci_client::client::ClientProtocol::Https;

    oci_client::client::ClientConfig {
        protocol,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::Path;

    use super::*;

    #[tokio::test]
    #[ignore = "requires external download"]
    async fn test_download_by_version() {
        let index =
            Repo::download_index(&Url::parse("oci://public.ecr.aws/karpenter").unwrap()).unwrap();

        let expected_hash =
            Sha256Hash::new("58e2b631389bdd232d50fd96dda52b085a7cc609079469e97a289f8e5c76657e");
        let cache = Cache::new(Path::new("tmp/oci_download_by_version"));
        let karpenter = index
            .get_by_version("karpenter", "1.0.5", &cache)
            .await
            .unwrap();

        assert_eq!(karpenter.meta.name, "karpenter");
        assert_eq!(karpenter.meta.version, "1.0.5");
        assert_eq!(karpenter.meta.sha256_hash, expected_hash);

        assert!(karpenter.file_path.exists());

        let hash = Sha256Hash::from_async_path(&karpenter.file_path)
            .await
            .unwrap();
        assert_eq!(hash, expected_hash);
    }

    #[tokio::test]
    #[ignore = "requires external download"]
    async fn test_download_by_meta() {
        let index =
            Repo::download_index(&Url::parse("oci://public.ecr.aws/karpenter").unwrap()).unwrap();

        let expected_hash =
            Sha256Hash::new("58e2b631389bdd232d50fd96dda52b085a7cc609079469e97a289f8e5c76657e");
        let meta = Meta {
            name: "karpenter".to_string(),
            version: "0.0.4".to_string(),
            sha256_hash: expected_hash.clone(),
            created: None,
            repo: RepoSpecific::Oci {
                manifest_digest:
                    "sha256:98382d6406a3c85711269112fbb337c056d4debabaefb936db2d10137b58bd1b"
                        .to_string(),
            },
            size: 36211,
        };

        let cache = Cache::new(Path::new("tmp/oci_download_by_meta"));
        let karpenter = index.get_by_meta(&meta, &cache).await.unwrap();

        assert_eq!(karpenter.meta.name, "karpenter");
        assert_eq!(karpenter.meta.version, "0.0.4");
        assert_eq!(karpenter.meta.sha256_hash, expected_hash);

        assert!(karpenter.file_path.exists());

        let hash = Sha256Hash::from_async_path(&karpenter.file_path)
            .await
            .unwrap();
        assert_eq!(hash, expected_hash);
    }
}
