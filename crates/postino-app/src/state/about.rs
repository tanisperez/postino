//! What Settings, "About" shows about the running build: version, platform, executable path and
//! the release links. Computed once (the executable path is a syscall), never in `render`.

use std::path::PathBuf;

use super::update::{CURRENT_VERSION, download_dir};

/// The repository page.
pub const REPO_URL: &str = "https://github.com/tanisperez/postino";

/// The release page of `version`.
pub fn release_notes_url(version: &str) -> String {
    format!("{REPO_URL}/releases/tag/v{version}")
}

/// The OS and architecture as shown to the user, for example "Linux x86_64" or "macOS arm64".
pub fn platform_label(os: &str, arch: &str) -> String {
    let (os, arch) = match (os, arch) {
        ("macos", "aarch64") => ("macOS", "arm64"),
        ("macos", arch) => ("macOS", arch),
        ("linux", arch) => ("Linux", arch),
        ("windows", arch) => ("Windows", arch),
        (os, arch) => (os, arch),
    };
    format!("{os} {arch}")
}

/// The facts "About" shows, gathered once.
#[derive(Debug, Clone)]
pub struct AboutInfo {
    /// The running version.
    pub version: &'static str,
    /// For example "Linux x86_64".
    pub platform: String,
    /// The running executable, `None` when the OS cannot tell.
    pub exe: Option<PathBuf>,
    /// Where updates are downloaded, `None` when the cache directory is unknown.
    pub download_dir: Option<PathBuf>,
}

impl AboutInfo {
    /// Reads the build and the environment.
    pub fn detect() -> Self {
        Self {
            version: CURRENT_VERSION,
            platform: platform_label(std::env::consts::OS, std::env::consts::ARCH),
            exe: std::env::current_exe()
                .ok()
                .map(|exe| dunce::canonicalize(&exe).unwrap_or(exe)),
            download_dir: download_dir(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_platform_is_readable() {
        assert_eq!(platform_label("linux", "x86_64"), "Linux x86_64");
        assert_eq!(platform_label("macos", "aarch64"), "macOS arm64");
        assert_eq!(platform_label("macos", "x86_64"), "macOS x86_64");
        assert_eq!(platform_label("windows", "x86_64"), "Windows x86_64");
        assert_eq!(platform_label("freebsd", "x86_64"), "freebsd x86_64");
    }

    #[test]
    fn the_release_notes_point_at_the_version_tag() {
        assert_eq!(
            release_notes_url("0.1.0"),
            "https://github.com/tanisperez/postino/releases/tag/v0.1.0"
        );
    }

    #[test]
    fn detect_reports_the_running_version() {
        assert_eq!(AboutInfo::detect().version, env!("CARGO_PKG_VERSION"));
    }
}
