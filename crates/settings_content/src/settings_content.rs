mod action;
mod editor;
mod fallible_options;
mod language;
pub mod merge_from;
mod project;
mod serde_helper;
mod theme;
mod title_bar;
mod workspace;

pub use action::{ActionName, ActionWithArguments};
pub use editor::*;
pub use fallible_options::*;
pub use language::*;
pub use merge_from::MergeFrom as MergeFromTrait;
pub use project::*;
use serde::de::DeserializeOwned;
pub use serde_helper::{
    serialize_f32_with_two_decimal_places, serialize_optional_f32_with_two_decimal_places,
};
use settings_json::parse_json_with_comments;
pub use theme::*;
pub use title_bar::*;
pub use workspace::*;

use collections::{HashMap, IndexMap};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};

/// Defines a settings override struct where each field is
/// `Option<Box<SettingsContent>>`, along with:
/// - `OVERRIDE_KEYS`: a `&[&str]` of the field names (the JSON keys)
/// - `get_by_key(&self, key) -> Option<&SettingsContent>`: accessor by key
///
/// The field list is the single source of truth for the override key strings.
macro_rules! settings_overrides {
    (
        $(#[$attr:meta])*
        pub struct $name:ident { $($field:ident),* $(,)? }
    ) => {
        $(#[$attr])*
        pub struct $name {
            $(pub $field: Option<Box<SettingsContent>>,)*
        }

        impl $name {
            /// The JSON override keys, derived from the field names on this struct.
            pub const OVERRIDE_KEYS: &[&str] = &[$(stringify!($field)),*];

            /// Look up an override by its JSON key name.
            pub fn get_by_key(&self, key: &str) -> Option<&SettingsContent> {
                match key {
                    $(stringify!($field) => self.$field.as_deref(),)*
                    _ => None,
                }
            }
        }
    }
}
use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hash;
pub use util::serde::default_true;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseStatus {
    /// Settings were parsed successfully
    Success,
    /// Settings file was not changed, so no parsing was performed
    Unchanged,
    /// Settings failed to parse
    Failed { error: String },
}

#[with_fallible_options]
#[derive(Debug, PartialEq, Default, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct SettingsContent {
    #[serde(flatten)]
    pub project: ProjectSettingsContent,

    #[serde(flatten)]
    pub theme: Box<ThemeSettingsContent>,

    #[serde(flatten)]
    pub workspace: WorkspaceSettingsContent,

    #[serde(flatten)]
    pub editor: EditorSettingsContent,

    #[serde(flatten)]
    pub remote: RemoteSettingsContent,

    /// Settings related to the file finder.
    pub file_finder: Option<FileFinderSettingsContent>,

    pub git_panel: Option<GitPanelSettingsContent>,

    pub tabs: Option<ItemSettingsContent>,
    pub tab_bar: Option<TabBarSettingsContent>,
    pub status_bar: Option<StatusBarSettingsContent>,

    pub preview_tabs: Option<PreviewTabsSettingsContent>,

    /// This base keymap settings adjusts the default keybindings in Zed to be similar
    /// to other common code editors. By default, Zed's keymap closely follows VSCode's
    /// keymap, with minor adjustments, this corresponds to the "VSCode" setting.
    ///
    /// Default: VSCode
    pub base_keymap: Option<BaseKeymapContent>,

    /// Configuration for Diagnostics-related features.
    pub diagnostics: Option<DiagnosticsSettingsContent>,

    /// Configuration for Git-related features
    pub git: Option<GitSettings>,

    /// Common language server settings.
    pub global_lsp_settings: Option<GlobalLspSettingsContent>,

    /// A map of log scopes to the desired log level.
    /// Useful for filtering out noisy logs or enabling more verbose logging.
    ///
    /// Example: {"log": {"client": "warn"}}
    pub log: Option<HashMap<String, String>>,

    pub line_indicator_format: Option<LineIndicatorFormat>,

    pub project_panel: Option<ProjectPanelSettingsContent>,

    /// Configuration for the Message Editor
    pub message_editor: Option<MessageEditorSettings>,

    /// Configuration for Node-related features
    pub node: Option<NodeBinarySettings>,

    pub proxy: Option<String>,

    /// Configuration for session-related features
    pub session: Option<SessionSettingsContent>,

    pub title_bar: Option<TitleBarSettingsContent>,

    /// Number of lines to search for modelines at the beginning and end of files.
    /// Modelines contain editor directives (e.g., vim/emacs settings) that configure
    /// the editor behavior for specific files.
    ///
    /// Default: 5
    pub modeline_lines: Option<usize>,

    /// Local overrides for feature flags, keyed by flag name.
    pub feature_flags: Option<FeatureFlagsMap>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, MergeFrom)]
#[serde(transparent)]
pub struct FeatureFlagsMap(pub HashMap<String, String>);

// A manual `JsonSchema` impl keeps this type's schema registered under a
// unique name. The derived impl on a `#[serde(transparent)]` newtype around
// `HashMap<String, String>` would inline to the map's own schema name (`Map_of_string`),
// which is shared with every other `HashMap<String, String>` setting field in
// `SettingsContent`. A named placeholder lets `json_schema_store` find and
// replace just this field's schema at runtime without clobbering the others.
impl JsonSchema for FeatureFlagsMap {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FeatureFlagsMap".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "additionalProperties": { "type": "string" }
        })
    }
}

impl std::ops::Deref for FeatureFlagsMap {
    type Target = HashMap<String, String>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for FeatureFlagsMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl SettingsContent {
    pub fn languages_mut(&mut self) -> &mut HashMap<String, LanguageSettingsContent> {
        &mut self.project.all_languages.languages.0
    }
}

// These impls are there to optimize builds by avoiding monomorphization downstream. Yes, they're repetitive, but using default impls
// break the optimization, for whatever reason.
pub trait RootUserSettings: Sized + DeserializeOwned {
    fn parse_json(json: &str) -> (Option<Self>, ParseStatus);
    fn parse_json_with_comments(json: &str) -> anyhow::Result<Self>;
}

impl RootUserSettings for SettingsContent {
    fn parse_json(json: &str) -> (Option<Self>, ParseStatus) {
        fallible_options::parse_json(json)
    }
    fn parse_json_with_comments(json: &str) -> anyhow::Result<Self> {
        parse_json_with_comments(json)
    }
}
// Explicit opt-in instead of blanket impl to avoid monomorphizing downstream. Just a hunch though.
impl RootUserSettings for Option<SettingsContent> {
    fn parse_json(json: &str) -> (Option<Self>, ParseStatus) {
        fallible_options::parse_json(json)
    }
    fn parse_json_with_comments(json: &str) -> anyhow::Result<Self> {
        parse_json_with_comments(json)
    }
}
impl RootUserSettings for UserSettingsContent {
    fn parse_json(json: &str) -> (Option<Self>, ParseStatus) {
        fallible_options::parse_json(json)
    }
    fn parse_json_with_comments(json: &str) -> anyhow::Result<Self> {
        parse_json_with_comments(json)
    }
}

settings_overrides! {
    #[with_fallible_options]
    #[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
    pub struct ReleaseChannelOverrides { dev, nightly, preview, stable }
}

settings_overrides! {
    #[with_fallible_options]
    #[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
    pub struct PlatformOverrides { macos, linux, windows }
}

/// Determines what settings a profile starts from before applying its overrides.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, MergeFrom,
)]
#[serde(rename_all = "snake_case")]
pub enum ProfileBase {
    /// Apply profile settings on top of the user's current settings.
    #[default]
    User,
    /// Apply profile settings on top of Zed's default settings, ignoring user customizations.
    Default,
}

/// A named settings profile that can temporarily override settings.
#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct SettingsProfile {
    /// What base settings to start from before applying this profile's overrides.
    ///
    /// - `user`: Apply on top of user's settings (default)
    /// - `default`: Apply on top of Zed's default settings, ignoring user customizations
    #[serde(default)]
    pub base: ProfileBase,

    /// The settings overrides for this profile.
    #[serde(default)]
    pub settings: Box<SettingsContent>,
}

#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct UserSettingsContent {
    #[serde(flatten)]
    pub content: Box<SettingsContent>,

    #[serde(flatten)]
    pub release_channel_overrides: ReleaseChannelOverrides,

    #[serde(flatten)]
    pub platform_overrides: PlatformOverrides,

    #[serde(default)]
    pub profiles: IndexMap<String, SettingsProfile>,
}

pub struct ExtensionsSettingsContent {
    pub all_languages: AllLanguageSettingsContent,
}

/// Base key bindings scheme. Base keymaps can be overridden with user keymaps.
///
/// Default: VSCode
#[derive(
    Copy,
    Clone,
    Debug,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    PartialEq,
    Eq,
    Default,
    strum::VariantArray,
)]
pub enum BaseKeymapContent {
    #[default]
    VSCode,
    JetBrains,
    SublimeText,
    Atom,
    TextMate,
    Emacs,
    Cursor,
    None,
}

impl strum::VariantNames for BaseKeymapContent {
    const VARIANTS: &'static [&'static str] = &[
        "VSCode",
        "JetBrains",
        "Sublime Text",
        "Atom",
        "TextMate",
        "Emacs",
        "Cursor",
        "None",
    ];
}

#[derive(
    Copy,
    Clone,
    Debug,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    PartialEq,
    Eq,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum DockPosition {
    Left,
    Bottom,
    Right,
}

#[with_fallible_options]
#[derive(Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema, MergeFrom, Debug)]
pub struct GitPanelSettingsContent {
    /// Whether to show the panel button in the status bar.
    ///
    /// Default: true
    pub button: Option<bool>,
    /// Where to dock the panel.
    ///
    /// Default: left
    pub dock: Option<DockPosition>,
    /// Default width of the panel in pixels.
    ///
    /// Default: 360
    #[serde(serialize_with = "crate::serialize_optional_f32_with_two_decimal_places")]
    pub default_width: Option<f32>,
    /// How entry statuses are displayed.
    ///
    /// Default: icon
    pub status_style: Option<StatusStyle>,

    /// Whether to show file icons in the git panel.
    ///
    /// Default: false
    pub file_icons: Option<bool>,

    /// Whether to show folder icons or chevrons for directories in the git panel.
    ///
    /// Default: true
    pub folder_icons: Option<bool>,

    /// How and when the scrollbar should be displayed.
    ///
    /// Default: inherits editor scrollbar settings
    pub scrollbar: Option<ScrollbarSettings>,

    /// What the default branch name should be when
    /// `init.defaultBranch` is not set in git
    ///
    /// Default: main
    pub fallback_branch_name: Option<String>,

    /// Whether to sort entries in the panel by path
    /// or by status (the default).
    ///
    /// Default: false
    pub sort_by_path: Option<bool>,

    /// Whether to collapse untracked files in the diff panel.
    ///
    /// Default: false
    pub collapse_untracked_diff: Option<bool>,

    /// Whether to show entries with tree or flat view in the panel
    ///
    /// Default: false
    pub tree_view: Option<bool>,

    /// Whether to show the addition/deletion change count next to each file in the Git panel.
    ///
    /// Default: true
    pub diff_stats: Option<bool>,

    /// Whether to show a badge on the git panel icon with the count of uncommitted changes.
    ///
    /// Default: false
    pub show_count_badge: Option<bool>,

    /// Whether the git panel should open on startup.
    ///
    /// Default: false
    pub starts_open: Option<bool>,

    /// Maximum length of the commit message title before a warning is shown.
    /// Set to 0 to disable.
    ///
    /// Default: 72
    pub commit_title_max_length: Option<usize>,
}

#[derive(
    Default,
    Copy,
    Clone,
    Debug,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    PartialEq,
    Eq,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum StatusStyle {
    #[default]
    Icon,
    LabelColor,
}

#[with_fallible_options]
#[derive(
    Copy, Clone, Default, Debug, Serialize, Deserialize, JsonSchema, MergeFrom, PartialEq, Eq,
)]
pub struct ScrollbarSettings {
    pub show: Option<ShowScrollbar>,
}

pub type ScrollbarSettingsContent = ScrollbarSettings;

/// When to show the scrollbar.
///
/// Default: auto
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    PartialEq,
    Eq,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum ShowScrollbar {
    /// Show the scrollbar if there's important information or
    /// follow the system's configured behavior.
    #[default]
    Auto,
    /// Match the system's configured behavior.
    System,
    /// Always show the scrollbar.
    Always,
    /// Never show the scrollbar.
    Never,
}

#[with_fallible_options]
#[derive(Clone, Default, Serialize, Deserialize, JsonSchema, MergeFrom, Debug, PartialEq)]
pub struct PanelSettingsContent {
    /// Whether to show the panel button in the status bar.
    ///
    /// Default: true
    pub button: Option<bool>,
    /// Where to dock the panel.
    ///
    /// Default: left
    pub dock: Option<DockPosition>,
    /// Default width of the panel in pixels.
    ///
    /// Default: 240
    #[serde(serialize_with = "crate::serialize_optional_f32_with_two_decimal_places")]
    pub default_width: Option<f32>,
}

#[with_fallible_options]
#[derive(Clone, Default, Serialize, Deserialize, JsonSchema, MergeFrom, Debug, PartialEq)]
pub struct MessageEditorSettings {
    /// Whether to automatically replace emoji shortcodes with emoji characters.
    /// For example: typing `:wave:` gets replaced with `👋`.
    ///
    /// Default: false
    pub auto_replace_emoji_shortcode: Option<bool>,
}

#[with_fallible_options]
#[derive(Clone, Default, Serialize, Deserialize, JsonSchema, MergeFrom, Debug, PartialEq)]
pub struct FileFinderSettingsContent {
    /// Whether to show file icons in the file finder.
    ///
    /// Default: true
    pub file_icons: Option<bool>,
    /// Determines how much space the file finder can take up in relation to the available window width.
    ///
    /// Default: small
    pub modal_max_width: Option<FileFinderWidthContent>,
    /// Determines whether the file finder should skip focus for the active file in search results.
    ///
    /// Default: true
    pub skip_focus_for_active_in_search: Option<bool>,
    /// Whether to use gitignored files when searching.
    /// Only the file Zed had indexed will be used, not necessary all the gitignored files.
    ///
    /// Default: Smart
    pub include_ignored: Option<IncludeIgnoredContent>,
    /// Whether to include text channels in file finder results.
    ///
    /// Default: false
    pub include_channels: Option<bool>,
}

#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    Copy,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum IncludeIgnoredContent {
    /// Use all gitignored files
    All,
    /// Use only the files Zed had indexed
    Indexed,
    /// Be smart and search for ignored when called from a gitignored worktree
    #[default]
    Smart,
}

#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    Copy,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "lowercase")]
pub enum FileFinderWidthContent {
    #[default]
    Small,
    Medium,
    Large,
    XLarge,
    Full,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum DockSide {
    Left,
    Right,
}

#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    JsonSchema,
    MergeFrom,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum ShowIndentGuides {
    Always,
    Never,
}

#[with_fallible_options]
#[derive(
    Copy, Clone, Debug, Serialize, Deserialize, JsonSchema, MergeFrom, PartialEq, Eq, Default,
)]
pub struct IndentGuidesSettingsContent {
    /// When to show the scrollbar in the outline panel.
    pub show: Option<ShowIndentGuides>,
}

#[derive(Clone, Copy, Default, PartialEq, Debug, JsonSchema, MergeFrom, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineIndicatorFormat {
    Short,
    #[default]
    Long,
}

#[with_fallible_options]
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, MergeFrom, PartialEq)]
pub struct RemoteSettingsContent {
    pub ssh_connections: Option<Vec<SshConnection>>,
    pub wsl_connections: Option<Vec<WslConnection>>,
    pub dev_container_connections: Option<Vec<DevContainerConnection>>,
    pub read_ssh_config: Option<bool>,
    pub use_podman: Option<bool>,
}

#[with_fallible_options]
#[derive(
    Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, MergeFrom, Hash,
)]
pub struct DevContainerConnection {
    pub name: String,
    pub remote_user: String,
    pub container_id: String,
    pub use_podman: bool,
    pub extension_ids: Vec<String>,
    pub remote_env: BTreeMap<String, String>,
}

#[with_fallible_options]
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, JsonSchema, MergeFrom)]
pub struct SshConnection {
    pub host: String,
    pub username: Option<String>,
    pub port: Option<u16>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub projects: collections::BTreeSet<RemoteProject>,
    /// Name to use for this server in UI.
    pub nickname: Option<String>,
    // By default Zed will download the binary to the host directly.
    // If this is set to true, Zed will download the binary to your local machine,
    // and then upload it over the SSH connection. Useful if your SSH server has
    // limited outbound internet access.
    pub upload_binary_over_ssh: Option<bool>,

    pub port_forwards: Option<Vec<SshPortForwardOption>>,
    /// Timeout in seconds for SSH connection and downloading the remote server binary.
    /// Defaults to 10 seconds if not specified.
    pub connection_timeout: Option<u16>,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, JsonSchema, MergeFrom, Debug)]
pub struct WslConnection {
    pub distro_name: String,
    pub user: Option<String>,
    #[serde(default)]
    pub projects: BTreeSet<RemoteProject>,
}

#[with_fallible_options]
#[derive(
    Clone, Debug, Default, Serialize, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema,
)]
pub struct RemoteProject {
    pub paths: Vec<String>,
}

#[with_fallible_options]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize, JsonSchema, MergeFrom)]
pub struct SshPortForwardOption {
    pub local_host: Option<String>,
    pub local_port: u16,
    pub remote_host: Option<String>,
    pub remote_port: u16,
}

// An ExtendingVec in the settings can only accumulate new values.
//
// This is useful for things like private files where you only want
// to allow new values to be added.
//
// Consider using a HashMap<String, bool> instead of this type so that user settings
// files can both add and remove values from the set.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExtendingVec<T>(pub Vec<T>);

impl<T> Into<Vec<T>> for ExtendingVec<T> {
    fn into(self) -> Vec<T> {
        self.0
    }
}
impl<T> From<Vec<T>> for ExtendingVec<T> {
    fn from(vec: Vec<T>) -> Self {
        ExtendingVec(vec)
    }
}

impl<T: Clone> merge_from::MergeFrom for ExtendingVec<T> {
    fn merge_from(&mut self, other: &Self) {
        self.0.extend_from_slice(other.0.as_slice());
    }
}

#[derive(
    Copy,
    Clone,
    Default,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    MergeFrom,
    JsonSchema,
    derive_more::FromStr,
)]
#[serde(transparent)]
pub struct DelayMs(pub u64);

impl From<u64> for DelayMs {
    fn from(n: u64) -> Self {
        Self(n)
    }
}

impl std::fmt::Display for DelayMs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}ms", self.0)
    }
}
