use semver::Version;

pub const APP_ID: &str = "app.zen.Zen";

#[cfg(target_os = "windows")]
pub fn app_identifier() -> &'static str {
    "Zen-Editor"
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

        let mut build_metadata = Vec::new();
        if let Some(build_id) = build_id {
            build_metadata.push(build_id.to_string());
        }
        if let Some(sha) = commit_sha {
            build_metadata.push(sha.0);
        }
        if !build_metadata.is_empty()
            && let Ok(build_metadata) = semver::BuildMetadata::new(&build_metadata.join("."))
        {
            version.build = build_metadata;
        }

        version
    }
}
