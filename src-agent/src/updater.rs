//! Self-update support: fetch the latest release binary from GitHub
//! and atomically replace the running executable.
//!
//! The update source follows the same asset naming convention produced by
//! `.github/workflows/release.yml` (`rupoo-<version>-<target>.tar.gz/.zip`),
//! so the GitHub backend can locate the archive for the current platform.

use self_update::Status;

const REPO_OWNER: &str = "Steventsang18";
const REPO_NAME: &str = "rupoo";
const BIN_NAME: &str = "rupoo";

/// Outcome of a self-update attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// The binary was replaced with the given new version.
    Updated(String),
    /// Already running the requested (or latest) version.
    UpToDate(String),
}

/// Check GitHub Releases and install the newest version of the rupoo binary.
///
/// The running executable is replaced atomically by `self_update`; a
/// restart is required for the new version to take effect.
///
/// # Errors
/// Returns a `String` describing the failure (network, archive, or
/// permission errors). This function never panics.
pub fn update() -> Result<UpdateOutcome, String> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .show_download_progress(true)
        .current_version(env!("CARGO_PKG_VERSION"))
        .build()
        .map_err(|e| e.to_string())?
        .update()
        .map_err(|e| e.to_string())?;

    Ok(match status {
        Status::Updated(v) => UpdateOutcome::Updated(v),
        Status::UpToDate(v) => UpdateOutcome::UpToDate(v),
    })
}

/// Check whether a newer release exists without installing it.
///
/// Returns `Ok(true)` when the latest GitHub release is newer than the
/// running version.
///
/// # Errors
/// Returns a `String` describing the failure (network or parse errors).
pub fn check() -> Result<bool, String> {
    let updater = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .current_version(env!("CARGO_PKG_VERSION"))
        .build()
        .map_err(|e| e.to_string())?;
    let latest = updater.get_latest_release().map_err(|e| e.to_string())?;
    Ok(version_newer_than(
        &latest.version,
        env!("CARGO_PKG_VERSION"),
    ))
}

/// Compare two dotted version strings ("0.6.3" vs "0.6.4").
/// Returns `true` when `a` is a higher version than `b`.
fn version_newer_than(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|p| p.parse::<u64>().ok())
            .collect()
    }
    parts(a) > parts(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_name_matches_crate() {
        // The update target must match the binary produced by release.yml.
        assert_eq!(BIN_NAME, "rupoo");
    }

    #[test]
    fn repo_points_to_published_location() {
        assert_eq!(REPO_OWNER, "Steventsang18");
        assert_eq!(REPO_NAME, "rupoo");
    }

    #[test]
    fn version_comparison_handles_patch_and_minor() {
        assert!(version_newer_than("0.6.4", "0.6.3"));
        assert!(version_newer_than("0.7.0", "0.6.9"));
        assert!(version_newer_than("v1.0.0", "0.9.9"));
        assert!(!version_newer_than("0.6.3", "0.6.3"));
        assert!(!version_newer_than("0.6.2", "0.6.3"));
    }
}
