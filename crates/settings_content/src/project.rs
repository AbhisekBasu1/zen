use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_json::parse_json_with_comments;
use settings_macros::{MergeFrom, with_fallible_options};

use crate::{
    AllLanguageSettingsContent, ExtendingVec, ParseStatus, RootUserSettings, fallible_options,
};

impl RootUserSettings for ProjectSettingsContent {
    fn parse_json(json: &str) -> (Option<Self>, ParseStatus) {
        fallible_options::parse_json(json)
    }
    fn parse_json_with_comments(json: &str) -> anyhow::Result<Self> {
        parse_json_with_comments(json)
    }
}

#[with_fallible_options]
#[derive(Debug, PartialEq, Clone, Default, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct ProjectSettingsContent {
    #[serde(flatten)]
    pub all_languages: AllLanguageSettingsContent,

    #[serde(flatten)]
    pub worktree: WorktreeSettingsContent,
}

#[with_fallible_options]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct WorktreeSettingsContent {
    /// Whether to prevent this project from being shared in public channels.
    ///
    /// Default: false
    #[serde(default)]
    pub prevent_sharing_in_public_channels: bool,

    /// Completely ignore files matching globs from `file_scan_exclusions`. Overrides
    /// `file_scan_inclusions`.
    ///
    /// Default: [
    ///   "**/.git",
    ///   "**/.svn",
    ///   "**/.hg",
    ///   "**/.jj",
    ///   "**/CVS",
    ///   "**/.DS_Store",
    ///   "**/Thumbs.db",
    ///   "**/.classpath",
    ///   "**/.settings"
    /// ]
    pub file_scan_exclusions: Option<Vec<String>>,

    /// Always include files that match these globs when scanning for files, even if they're
    /// ignored by git. This setting is overridden by `file_scan_exclusions`.
    /// Default: [
    ///  ".env*",
    ///  "docker-compose.*.yml",
    /// ]
    pub file_scan_inclusions: Option<Vec<String>>,

    /// Treat the files matching these globs as `.env` files.
    /// Default: ["**/.env*", "**/*.pem", "**/*.key", "**/*.cert", "**/*.crt", "**/secrets.yml"]
    pub private_files: Option<ExtendingVec<String>>,

    /// Treat the files matching these globs as hidden files. You can hide hidden files in the project panel.
    /// Default: ["**/.*"]
    pub hidden_files: Option<Vec<String>>,

    /// Treat the files matching these globs as read-only. These files can be opened and viewed,
    /// but cannot be edited. This is useful for generated files, build outputs, or files from
    /// external dependencies that should not be modified directly.
    /// Default: []
    pub read_only_files: Option<Vec<String>>,
}

#[with_fallible_options]
#[derive(
    Default, Copy, Clone, PartialEq, Eq, Debug, Serialize, Deserialize, JsonSchema, MergeFrom,
)]
pub struct SessionSettingsContent {
    /// Whether or not to restore unsaved buffers on restart.
    ///
    /// If this is true, user won't be prompted whether to save/discard
    /// dirty files when closing the application.
    ///
    /// Default: true
    pub restore_unsaved_buffers: Option<bool>,
    /// Whether or not to skip worktree trust checks.
    /// When trusted, project settings are synchronized automatically,
    /// language and MCP servers are downloaded and started automatically.
    ///
    /// Default: false
    pub trust_all_worktrees: Option<bool>,
}
