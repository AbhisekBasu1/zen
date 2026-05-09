use gpui::{Action, actions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// If the zen binary doesn't use anything in this crate, it will be optimized away
// and the actions won't initialize. So we just provide an empty initialization function
// to be called from main.
//
// These may provide relevant context:
// https://github.com/rust-lang/rust/issues/47384
// https://github.com/mmastrac/rust-ctor/issues/280
pub fn init() {}

/// Opens a URL in the system's default web browser.
#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = zen)]
#[serde(deny_unknown_fields)]
pub struct OpenBrowser {
    pub url: String,
}

/// Opens a zen:// URL within the application.
#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = zen)]
#[serde(deny_unknown_fields)]
pub struct OpenZenUrl {
    pub url: String,
}

/// Opens the keymap to either add a keybinding or change an existing one
#[derive(PartialEq, Clone, Default, Action, JsonSchema, Serialize, Deserialize)]
#[action(namespace = zen, no_json, no_register)]
pub struct ChangeKeybinding {
    pub action: String,
}

actions!(
    zen,
    [
        /// Opens the settings editor.
        #[action(deprecated_aliases = ["zen_actions::OpenSettingsEditor"])]
        OpenSettings,
        /// Opens the settings JSON file.
        #[action(deprecated_aliases = ["zen_actions::OpenSettings"])]
        OpenSettingsFile,
        /// Opens project-specific settings.
        #[action(deprecated_aliases = ["zen_actions::OpenProjectSettings"])]
        OpenProjectSettings,
        /// Opens the default keymap file.
        OpenDefaultKeymap,
        /// Opens the user keymap file.
        #[action(deprecated_aliases = ["zen_actions::OpenKeymap"])]
        OpenKeymapFile,
        /// Opens the keymap editor.
        #[action(deprecated_aliases = ["zen_actions::OpenKeymapEditor"])]
        OpenKeymap,
        /// Quits the application.
        Quit,
        /// Shows information about Zen.
        About,
    ]
);

/// Decreases the font size in the editor buffer.
#[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = zen)]
#[serde(deny_unknown_fields)]
pub struct DecreaseBufferFontSize {
    #[serde(default)]
    pub persist: bool,
}

/// Increases the font size in the editor buffer.
#[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = zen)]
#[serde(deny_unknown_fields)]
pub struct IncreaseBufferFontSize {
    #[serde(default)]
    pub persist: bool,
}

/// Opens the settings editor at a specific path.
#[derive(PartialEq, Clone, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = zen)]
#[serde(deny_unknown_fields)]
pub struct OpenSettingsAt {
    /// A path to a specific setting (e.g. `theme.mode`)
    pub path: String,
}

/// Resets the buffer font size to the default value.
#[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = zen)]
#[serde(deny_unknown_fields)]
pub struct ResetBufferFontSize {
    #[serde(default)]
    pub persist: bool,
}

/// Decreases the font size of the user interface.
#[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = zen)]
#[serde(deny_unknown_fields)]
pub struct DecreaseUiFontSize {
    #[serde(default)]
    pub persist: bool,
}

/// Increases the font size of the user interface.
#[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = zen)]
#[serde(deny_unknown_fields)]
pub struct IncreaseUiFontSize {
    #[serde(default)]
    pub persist: bool,
}

/// Resets the UI font size to the default value.
#[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = zen)]
#[serde(deny_unknown_fields)]
pub struct ResetUiFontSize {
    #[serde(default)]
    pub persist: bool,
}

/// Resets all zoom levels (UI and buffer font sizes) to their default values.
#[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = zen)]
#[serde(deny_unknown_fields)]
pub struct ResetAllZoom {
    #[serde(default)]
    pub persist: bool,
}

pub mod editor {
    use gpui::actions;
    actions!(
        editor,
        [
            /// Moves cursor up.
            MoveUp,
            /// Moves cursor down.
            MoveDown,
            /// Reveals the current file in the system file manager.
            RevealInFileManager,
        ]
    );
}

pub mod workspace {
    use gpui::actions;

    actions!(
        workspace,
        [
            #[action(deprecated_aliases = ["editor::CopyPath", "outline_panel::CopyPath", "project_panel::CopyPath"])]
            CopyPath,
            #[action(deprecated_aliases = ["editor::CopyRelativePath", "outline_panel::CopyRelativePath", "project_panel::CopyRelativePath"])]
            CopyRelativePath,
            /// Opens the selected file with the system's default application.
            #[action(deprecated_aliases = ["project_panel::OpenWithSystem"])]
            OpenWithSystem,
        ]
    );
}

/// Describes which ref to base a new git worktree on. The worktree is
/// always created in a detached HEAD state; users can opt into creating
/// a branch afterwards from the worktree itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum NewWorktreeBranchTarget {
    /// Create a detached worktree from the current HEAD.
    #[default]
    CurrentBranch,
    /// Create a detached worktree at the tip of an existing branch.
    ExistingBranch { name: String },
}

/// Creates a new git worktree and switches the workspace to it.
/// Dispatched by the unified worktree picker when the user selects a "Create new worktree" entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Action)]
#[action(namespace = git)]
#[serde(deny_unknown_fields)]
pub struct CreateWorktree {
    /// When this is None, Zen will randomly generate a worktree name.
    pub worktree_name: Option<String>,
    pub branch_target: NewWorktreeBranchTarget,
}

/// Switches the workspace to an existing linked worktree.
/// Dispatched by the unified worktree picker when the user selects an existing worktree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Action)]
#[action(namespace = git)]
#[serde(deny_unknown_fields)]
pub struct SwitchWorktree {
    pub path: PathBuf,
    pub display_name: String,
}

/// Opens an existing worktree in a new window.
/// Dispatched by the worktree picker's "Open in New Window" button.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Action)]
#[action(namespace = git)]
#[serde(deny_unknown_fields)]
pub struct OpenWorktreeInNewWindow {
    pub path: PathBuf,
}

pub mod git {
    use gpui::actions;

    actions!(
        git,
        [
            /// Checks out a different git branch.
            CheckoutBranch,
            /// Switches to a different git branch.
            Switch,
            /// Selects a different repository.
            SelectRepo,
            /// Filter remotes.
            FilterRemotes,
            /// Create a git remote.
            CreateRemote,
            /// Opens the git branch selector.
            #[action(deprecated_aliases = ["branches::OpenRecent"])]
            Branch,
            /// Opens the git stash selector.
            ViewStash,
            /// Opens the git worktree selector.
            Worktree,
            /// Creates a pull request for the current branch.
            CreatePullRequest
        ]
    );
}

pub mod toast {
    use gpui::actions;

    actions!(
        toast,
        [
            /// Runs the action associated with a toast notification.
            RunAction
        ]
    );
}

pub mod project_panel {
    use gpui::actions;

    actions!(
        project_panel,
        [
            /// Toggles the project panel.
            Toggle,
            /// Toggles focus on the project panel.
            ToggleFocus
        ]
    );
}
pub mod theme {
    use gpui::actions;

    actions!(theme, [ToggleMode]);
}

pub mod search {
    use gpui::actions;
    actions!(
        search,
        [
            /// Toggles searching in ignored files.
            ToggleIncludeIgnored
        ]
    );
}
pub mod buffer_search {
    use gpui::{Action, actions};
    use schemars::JsonSchema;
    use serde::Deserialize;

    /// Opens the buffer search interface with the specified configuration.
    #[derive(PartialEq, Clone, Deserialize, JsonSchema, Action)]
    #[action(namespace = buffer_search)]
    #[serde(deny_unknown_fields)]
    pub struct Deploy {
        #[serde(default = "util::serde::default_true")]
        pub focus: bool,
        #[serde(default)]
        pub replace_enabled: bool,
        #[serde(default)]
        pub selection_search_enabled: bool,
    }

    impl Deploy {
        pub fn find() -> Self {
            Self {
                focus: true,
                replace_enabled: false,
                selection_search_enabled: false,
            }
        }

        pub fn replace() -> Self {
            Self {
                focus: true,
                replace_enabled: true,
                selection_search_enabled: false,
            }
        }
    }

    actions!(
        buffer_search,
        [
            /// Deploys the search and replace interface.
            DeployReplace,
            /// Dismisses the search bar.
            Dismiss,
            /// Focuses back on the editor.
            FocusEditor,
            /// Sets the search query to the current selection without opening the search bar or running a search.
            UseSelectionForFind,
        ]
    );
}
/// Opens the recent projects interface.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = projects)]
#[serde(deny_unknown_fields)]
pub struct OpenRecent {
    #[serde(default)]
    pub create_new_window: bool,
}

pub mod outline {
    use gpui::actions;

    actions!(
        outline,
        [
            #[action(name = "Toggle")]
            ToggleOutline
        ]
    );
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WslConnectionOptions {
    pub distro_name: String,
    pub user: Option<String>,
}

#[cfg(target_os = "windows")]
pub mod wsl_actions {
    use gpui::Action;
    use schemars::JsonSchema;
    use serde::Deserialize;

    /// Opens a folder inside Wsl.
    #[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
    #[action(namespace = projects)]
    #[serde(deny_unknown_fields)]
    pub struct OpenFolderInWsl {
        #[serde(default)]
        pub create_new_window: bool,
    }

    /// Open a wsl distro.
    #[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
    #[action(namespace = projects)]
    #[serde(deny_unknown_fields)]
    pub struct OpenWsl {
        #[serde(default)]
        pub create_new_window: bool,
    }
}

pub mod preview {
    pub mod markdown {
        use gpui::actions;

        actions!(
            markdown,
            [
                /// Opens a markdown preview for the current file.
                OpenPreview,
                /// Opens a markdown preview in a split pane.
                OpenPreviewToTheSide,
                /// Toggles a markdown preview for the current file.
                TogglePreview,
            ]
        );
    }
}
