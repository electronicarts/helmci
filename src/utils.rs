// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.

use std::{error::Error, fmt::Display, path::Path};

#[derive(Debug)]
pub struct UnicodeError {}

impl Display for UnicodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Unicode Error\n")
    }
}
impl Error for UnicodeError {}

#[derive(Debug)]
pub enum FilenameError {
    UnicodeError(UnicodeError),
    IllegalFilename(),
}

impl Display for FilenameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilenameError::UnicodeError(_err) => f.write_str("Unicode Error\n"),
            FilenameError::IllegalFilename() => f.write_str("Illegal Filename\n"),
        }
    }
}
impl Error for FilenameError {}

impl From<UnicodeError> for FilenameError {
    fn from(err: UnicodeError) -> Self {
        FilenameError::UnicodeError(err)
    }
}

pub fn path_to_string(path: &Path) -> Result<String, FilenameError> {
    let result = path.to_str().ok_or(UnicodeError {})?.to_string();
    Ok(result)
}

pub fn filename_to_string(path: &Path) -> Result<String, FilenameError> {
    let result = path
        .file_name()
        .ok_or(FilenameError::IllegalFilename())?
        .to_str()
        .ok_or(UnicodeError {})?
        .to_string();
    Ok(result)
}
