use super::meta::Meta;
use std::fmt::Debug;

#[derive(Debug)]
pub struct Chart {
    pub file_path: std::path::PathBuf,
    pub meta: Meta,
}
