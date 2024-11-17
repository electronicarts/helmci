use thiserror::Error;
use url::Url;

#[derive(Error, Debug)]
pub enum AppendUrlError {
    #[error("Failed to split path into segments")]
    UrlJoin,
}

/// This function appends a path to a URL
///
/// This is required, because url.join has non-desirable
/// behavior when the URL does not end in a slash.
///
/// See <https://github.com/servo/rust-url/pull/934>
pub fn append_url(url: &Url, path: &str) -> Result<Url, AppendUrlError> {
    let mut url = url.clone();
    {
        let mut path_segments = url
            .path_segments_mut()
            .map_err(|()| AppendUrlError::UrlJoin)?;
        path_segments.pop_if_empty();
        for segment in path.split('/') {
            path_segments.push(segment);
        }
    }
    Ok(url)
}
