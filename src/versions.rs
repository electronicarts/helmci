use tap::Pipe;

pub type Error = semver::Error;
pub type Version = semver::Version;

/// Parse a semver complaint version.
pub fn parse_version(tag: &str) -> Result<Version, Error> {
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(tag)?.pipe(Ok)
}
