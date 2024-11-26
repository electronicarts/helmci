use std::fmt::Debug;
use std::io::SeekFrom;
use std::path::Path;
use std::path::PathBuf;

use super::hash::Sha256Hash;
use async_std::fs::File;
use async_std::io::SeekExt;
use async_std::io::WriteExt;
use tap::Pipe;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("SHA256 hash mismatch {0}: {1} != {2}")]
    Sha256HashMismatch(PathBuf, Sha256Hash, Sha256Hash),
    #[error("File too big error {0}: expected {1} got {2}")]
    TooBig(PathBuf, u64, u64),
}

#[derive(Debug)]
pub struct Cache {
    pub path: std::path::PathBuf,
}

#[derive(Clone, Debug)]
pub(super) struct Key {
    pub name: String,
    pub sha256_hash: Sha256Hash,
}

pub(super) struct CreateCacheEntry {
    path: PathBuf,
    temp_path: PathBuf,
    file: File,
    expected_size: Option<u64>,
}

impl CreateCacheEntry {
    async fn new(
        path: PathBuf,
        temp_path: PathBuf,
        expected_size: Option<u64>,
    ) -> Result<Self, Error> {
        let parent = temp_path.parent().ok_or_else(|| {
            Error::Io(
                temp_path.clone(),
                std::io::Error::new(std::io::ErrorKind::NotFound, "No parent directory"),
            )
        })?;

        tokio::fs::create_dir_all(&parent)
            .await
            .map_err(|e| Error::Io(parent.into(), e))?;

        let file = File::create(&temp_path)
            .await
            .map_err(|e| Error::Io(temp_path.clone(), e))?;

        Self {
            path,
            temp_path,
            file,
            expected_size,
        }
        .pipe(Ok)
    }

    pub(super) async fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), Error> {
        self.file
            .write_all(chunk)
            .await
            .map_err(|e| Error::Io(self.temp_path.clone(), e))?;

        if let Some(expected_size) = self.expected_size {
            let file_len = self
                .file
                .seek(SeekFrom::End(0))
                .await
                .map_err(|e| Error::Io(self.temp_path.clone(), e))?;

            if file_len > expected_size {
                return Err(Error::TooBig(
                    self.temp_path.clone(),
                    expected_size,
                    file_len,
                ));
            }
        }

        Ok(())
    }

    // pub(super) const fn file(&self) -> &File {
    //     &self.file
    // }

    pub(super) fn mut_file(&mut self) -> &mut File {
        &mut self.file
    }

    fn get_cache_file_path(&self, key: &Key) -> PathBuf {
        self.path
            .join(&key.name)
            .join(format!("{}.tgz", key.sha256_hash))
    }

    pub(super) async fn finalize(mut self, key: &Key) -> Result<PathBuf, Error> {
        let final_path = self.get_cache_file_path(key);

        self.flush().await?;

        let sha256_hash = self.calc_sha256_hash().await?;
        if key.sha256_hash != sha256_hash {
            return Err(Error::Sha256HashMismatch(
                self.temp_path.clone(),
                key.sha256_hash.clone(),
                sha256_hash,
            ));
        }

        let parent = final_path.parent().ok_or_else(|| {
            Error::Io(
                final_path.clone(),
                std::io::Error::new(std::io::ErrorKind::NotFound, "No parent directory"),
            )
        })?;

        tokio::fs::create_dir_all(&parent)
            .await
            .map_err(|e| Error::Io(parent.into(), e))?;

        tokio::fs::rename(&self.temp_path, &final_path)
            .await
            .map_err(|e| Error::Io(final_path.clone(), e))?;

        Ok(final_path.clone())
    }

    pub(super) async fn flush(&mut self) -> Result<(), Error> {
        self.file
            .flush()
            .await
            .map_err(|e| Error::Io(self.temp_path.clone(), e))
    }

    pub(super) async fn calc_sha256_hash(&self) -> Result<Sha256Hash, Error> {
        Sha256Hash::from_async_path(&self.temp_path)
            .await
            .map_err(|e| Error::Io(self.temp_path.clone(), e))
    }
}

impl Drop for CreateCacheEntry {
    fn drop(&mut self) {
        if self.temp_path.exists() {
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}

// Cache is not thread safe as creating entries will have race conditions.
unsafe impl Sync for Cache {}

impl Cache {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.join(".charts"),
        }
    }

    fn get_cache_file_path(&self, key: &Key) -> PathBuf {
        self.path
            .join(&key.name)
            .join(format!("{}.tgz", key.sha256_hash))
    }

    pub(super) async fn get_cache_entry(&self, key: &Key) -> Result<Option<PathBuf>, Error> {
        let file_path = self.get_cache_file_path(key);

        if !file_path.exists() {
            return Ok(None);
        }

        let sha256_hash = Sha256Hash::from_async_path(&file_path)
            .await
            .map_err(|e| Error::Io(file_path.clone(), e))?;

        if key.sha256_hash != sha256_hash {
            return Err(Error::Sha256HashMismatch(
                file_path.clone(),
                key.sha256_hash.clone(),
                sha256_hash,
            ));
        }

        Ok(Some(file_path))
    }

    pub(super) async fn create_cache_entry(
        &self,
        extension: &str,
        expected_size: Option<u64>,
    ) -> Result<CreateCacheEntry, Error> {
        let temp_path = self.path.join(format!("tmp{extension}"));
        CreateCacheEntry::new(self.path.clone(), temp_path, expected_size).await
    }
}
