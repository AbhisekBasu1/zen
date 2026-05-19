use gpui::{Action, actions};
use schemars::JsonSchema;
use serde::Deserialize;

// If the zen binary doesn't use anything in this crate, it will be optimized away
// and the actions won't initialize. So we just provide an empty initialization function
// to be called from main.
//
// These may provide relevant context:
// https://github.com/rust-lang/rust/issues/47384
// https://github.com/mmastrac/rust-ctor/issues/280
pub fn init() {}

actions!(
    zen,
    [
        /// Quits the application.
        Quit,
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
            /// Marks the selected text as agent-authored.
            MarkSelectionAsAgent,
            /// Marks the selected text as human-authored.
            MarkSelectionAsHuman,
            /// Moves cursor up.
            MoveUp,
            /// Moves cursor down.
            MoveDown,
            /// Pastes clipboard contents as agent-authored text.
            PasteAsAgent,
            /// Reveals the current file in the system file manager.
            RevealInFileManager,
            /// Toggles human authorship highlights in the active editor.
            ToggleAuthorship,
        ]
    );
}

pub mod workspace {
    use gpui::actions;

    actions!(
        workspace,
        [
            CopyPath,
            CopyRelativePath,
            /// Opens the selected file with the system's default application.
            OpenWithSystem,
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

    actions!(
        theme,
        [
            /// Selects the active theme.
            Select,
            /// Toggles between light and dark theme mode.
            ToggleMode
        ]
    );
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
