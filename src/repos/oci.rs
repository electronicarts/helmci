use docker_credential::{CredentialRetrievalError, DockerCredential};
use oci_client::{manifest::OciManifest, secrets::RegistryAuth, Client, Reference};
use thiserror::Error;
use url::Url;

use crate::repos::{hash::Sha256Hash, meta::RepoSpecific};

use super::{
    cache::{self, Cache},
    charts::Chart,
    meta::Meta,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Cache error: {0}")]
    Cache(#[from] cache::Error),
    #[error("Cache streaming error: {0}")]
    CacheStream(#[from] cache::StreamError<std::io::Error>),
    #[error("Invalid OCI URL")]
    InvalidOciUrl,
    #[error("OCI parse error: {0}")]
    OciParseClient(#[from] oci_client::ParseError),
    #[error("OCI error: {0}")]
    OciDistribution(#[from] oci_client::errors::OciDistributionError),
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
}

#[allow(dead_code)]
pub struct Repo {
    host: String,
    path: String,
}

impl Repo {
    pub fn download_index(url: &Url) -> Result<Self, Error> {
        if url.scheme() != "oci" {
            return Err(Error::InvalidOciUrl);
        }

        let host = url.host_str().ok_or(Error::InvalidOciUrl)?.to_string();
        let path = String::from(url.path());

        Ok(Repo { host, path })
    }

    async fn get_by_reference(
        &self,
        reference: &Reference,
        cache: &Cache,
        name: &str,
        version: &str,
    ) -> Result<Chart, Error> {
        let auth = build_auth(reference)?;
        let client_config = build_client_config();
        let client = Client::new(client_config);

        let (manifest, manifest_digest) = client.pull_manifest(reference, &auth).await?;

        let OciManifest::Image(manifest) = manifest else {
            return Err(Error::NotAnOciImage);
        };

        let [layer] = manifest.layers.as_slice() else {
            return Err(Error::NotAnOciImage);
        };

        let Some(hash) = layer.digest.strip_prefix("sha256:") else {
            return Err(Error::NotAnOciImage);
        };

        let annotations = manifest.annotations;

        // Cam't rely on annotations being present
        // let Some(name) = annotations.get("org.opencontainers.image.title") else {
        //     return Err(Error::MissingAnnotation(
        //         "org.opencontainers.image.title".to_string(),
        //     ));
        // };

        // Cam't rely on annotations being present
        // let Some(version) = annotations.get("org.opencontainers.image.version") else {
        //     return Err(Error::MissingAnnotation(
        //         "org.opencontainers.image.version".to_string(),
        //     ));
        // };

        let created = annotations
            .as_ref()
            .and_then(|a| a.get("org.opencontainers.image.created"))
            .and_then(|date| date.parse().ok());

        let chart = Meta {
            name: name.to_string(),
            version: version.to_string(),
            sha256_hash: Sha256Hash::new(hash),
            created,
            repo: RepoSpecific::Oci {
                digest: manifest_digest,
            },
        };

        if let Some(entry) = cache.get_cache_entry(&chart).await? {
            return Ok(entry);
        }

        let mut stream = client.pull_blob_stream(reference, &layer).await?;
        let cache = cache.create_cache_entry(chart, &mut stream).await?;
        Ok(cache)
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
        self.get_by_reference(&reference, cache, name, version)
            .await
    }

    pub async fn get_by_meta(&self, meta: &Meta, cache: &Cache) -> Result<Chart, Error> {
        let path = format!("{}/{}", self.path, meta.name);
        let RepoSpecific::Oci { digest } = &meta.repo else {
            return Err(Error::NotAnOciImage);
        };

        let reference: Reference =
            Reference::with_digest(self.host.clone(), path, digest.to_string());
        self.get_by_reference(&reference, cache, &meta.name, &meta.version)
            .await
    }
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
    #[ignore]
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

        let hash = Sha256Hash::from_file_async(&karpenter.file_path)
            .await
            .unwrap();
        assert_eq!(hash, expected_hash);
    }

    #[tokio::test]
    #[ignore]
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
                digest: "sha256:98382d6406a3c85711269112fbb337c056d4debabaefb936db2d10137b58bd1b"
                    .to_string(),
            },
        };

        let cache = Cache::new(Path::new("tmp/oci_download_by_meta"));
        let karpenter = index.get_by_meta(&meta, &cache).await.unwrap();

        assert_eq!(karpenter.meta.name, "karpenter");
        assert_eq!(karpenter.meta.version, "0.0.4");
        assert_eq!(karpenter.meta.sha256_hash, expected_hash);

        assert!(karpenter.file_path.exists());

        let hash = Sha256Hash::from_file_async(&karpenter.file_path)
            .await
            .unwrap();
        assert_eq!(hash, expected_hash);
    }
}
