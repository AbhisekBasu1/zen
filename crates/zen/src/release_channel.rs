use std::sync::LazyLock;

use gpui::{App, Global};
use semver::Version;

pub static RELEASE_CHANNEL: LazyLock<ReleaseChannel> = LazyLock::new(|| ReleaseChannel::Dev);

#[cfg(target_os = "windows")]
pub fn app_identifier() -> &'static str {
    "Zen-Editor-Dev"
}

#[derive(Clone, Eq, Debug, PartialEq)]
pub struct AppCommitSha(String);

impl AppCommitSha {
    pub fn new(sha: String) -> Self {
        AppCommitSha(sha)
    }

    pub fn short(&self) -> String {
        self.0.chars().take(7).collect()
    }
}

pub struct AppVersion;

impl AppVersion {
    pub fn load(
        pkg_version: &str,
        build_id: Option<&str>,
        commit_sha: Option<AppCommitSha>,
    ) -> Version {
        let mut version: Version = std::env::var("ZEN_APP_VERSION")
            .ok()
            .and_then(|from_env| from_env.parse().ok())
            .or_else(|| pkg_version.parse().ok())
            .unwrap_or_else(|| Version::new(0, 0, 0));

        let mut build_metadata = String::from(RELEASE_CHANNEL.dev_name());
        if let Some(build_id) = build_id {
            build_metadata.push('.');
            build_metadata.push_str(build_id);
        }
        if let Some(sha) = commit_sha {
            build_metadata.push('.');
            build_metadata.push_str(&sha.0);
        }
        if let Ok(build_metadata) = semver::BuildMetadata::new(&build_metadata) {
            version.build = build_metadata;
        }

        version
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ReleaseChannel {
    #[default]
    Dev,
}

struct GlobalReleaseChannel(ReleaseChannel);

impl Global for GlobalReleaseChannel {}

pub fn init(_app_version: Version, cx: &mut App) {
    cx.set_global(GlobalReleaseChannel(*RELEASE_CHANNEL))
}

impl ReleaseChannel {
    pub fn global(cx: &App) -> Self {
        cx.try_global::<GlobalReleaseChannel>()
            .map(|channel| channel.0)
            .unwrap_or_default()
    }

    pub fn dev_name(&self) -> &'static str {
        "dev"
    }

    pub fn app_id(&self) -> &'static str {
        "app.zen.Zen-Dev"
    }
}
