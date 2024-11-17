use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::meta::Meta;

#[derive(Error, Debug)]
pub enum Error {
    #[error("error with file {filename}: {error}")]
    Io {
        filename: PathBuf,
        error: std::io::Error,
    },
}

#[derive(Serialize, Deserialize)]
pub struct Lock {
    pub meta: Meta,
}

impl Lock {
    pub const fn new(meta: Meta) -> Self {
        Self { meta }
    }

    pub fn _load(lock_file: &std::path::Path) -> Result<Self, std::io::Error> {
        let file: String = std::fs::read_to_string(lock_file)?;
        let locks: Lock = serde_json::from_str(&file)?;
        Ok(locks)
    }

    pub fn load(lock_file: &std::path::Path) -> Result<Self, Error> {
        Self::_load(lock_file).map_err(|err| Error::Io {
            filename: lock_file.to_path_buf(),
            error: err,
        })
    }

    pub fn save(&self, lock_file: &std::path::Path) -> Result<(), std::io::Error> {
        let file = serde_json::to_string(&self)?;
        std::fs::write(lock_file, file)?;
        Ok(())
    }
}
