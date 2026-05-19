use std::fmt::Display;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};

use crate::{DelayMs, ShowScrollbar, serialize_f32_with_two_decimal_places};

#[with_fallible_options]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct EditorSettingsContent {
    /// Whether the cursor blinks in the editor.
    ///
    /// Default: true
    pub cursor_blink: Option<bool>,
    /// Cursor shape for the default editor.
    /// Can be "bar", "block", "underline", or "hollow".
    ///
    /// Default: bar
    pub cursor_shape: Option<CursorShape>,
    /// Determines when the mouse cursor should be hidden in an editor or input box.
    ///
    /// Default: on_typing_and_movement
    pub hide_mouse: Option<HideMouseMode>,
    /// How to highlight the current line in the editor.
    ///
    /// Default: all
    pub current_line_highlight: Option<CurrentLineHighlight>,
    /// Whether to highlight all occurrences of the selected text in an editor.
    ///
    /// Default: true
    pub selection_highlight: Option<bool>,
    /// Whether the text selection should have rounded corners.
    ///
    /// Default: true
    pub rounded_selection: Option<bool>,
    /// Toolbar related settings
    pub toolbar: Option<ToolbarContent>,
    /// Scrollbar related settings
    pub scrollbar: Option<ScrollbarContent>,
    /// Gutter related settings
    pub gutter: Option<GutterContent>,
    /// Whether the editor will scroll beyond the last line.
    ///
    /// Default: one_page
    pub scroll_beyond_last_line: Option<ScrollBeyondLastLine>,
    /// The number of lines to keep above/below the cursor when auto-scrolling.
    ///
    /// Default: 3.
    #[serde(serialize_with = "crate::serialize_optional_f32_with_two_decimal_places")]
    pub vertical_scroll_margin: Option<f32>,
    /// Whether to scroll when clicking near the edge of the visible text area.
    ///
    /// Default: false
    pub autoscroll_on_clicks: Option<bool>,
    /// The number of characters to keep on either side when scrolling with the mouse.
    ///
    /// Default: 5.
    #[serde(serialize_with = "crate::serialize_optional_f32_with_two_decimal_places")]
    pub horizontal_scroll_margin: Option<f32>,
    /// Scroll sensitivity multiplier. This multiplier is applied
    /// to both the horizontal and vertical delta values while scrolling.
    ///
    /// Default: 1.0
    #[serde(serialize_with = "crate::serialize_optional_f32_with_two_decimal_places")]
    pub scroll_sensitivity: Option<f32>,
    /// Whether to zoom the editor font size with the mouse wheel
    /// while holding the primary modifier key (Cmd on macOS, Ctrl on other platforms).
    ///
    /// Default: false
    pub mouse_wheel_zoom: Option<bool>,
    /// Scroll sensitivity multiplier for fast scrolling. This multiplier is applied
    /// to both the horizontal and vertical delta values while scrolling. Fast scrolling
    /// happens when a user holds the alt or option key while scrolling.
    ///
    /// Default: 4.0
    #[serde(serialize_with = "crate::serialize_optional_f32_with_two_decimal_places")]
    pub fast_scroll_sensitivity: Option<f32>,
    /// Settings for sticking scopes to the top of the editor.
    ///
    /// Default: sticky scroll is disabled
    pub sticky_scroll: Option<StickyScrollContent>,
    /// Whether the line numbers on editors gutter are relative or not.
    /// When "enabled" shows relative number of buffer lines, when "wrapped" shows
    /// relative number of display lines.
    ///
    /// Default: "disabled"
    pub relative_line_numbers: Option<RelativeLineNumbers>,
    /// Determines the modifier to be used to add multiple cursors with the mouse. The open hover link mouse gestures will adapt such that it do not conflict with the multicursor modifier.
    ///
    /// Default: alt
    pub multi_cursor_modifier: Option<MultiCursorModifier>,
    /// Hide the values of variables in `private` files, as defined by the
    /// private_files setting. This only changes the visual representation,
    /// the values are still present in the file and can be selected / copied / pasted
    ///
    /// Default: false
    pub redact_private_values: Option<bool>,

    /// How many lines to expand the multibuffer excerpts by default
    ///
    /// Default: 3
    pub expand_excerpt_lines: Option<u32>,

    /// How many lines of context to provide in multibuffer excerpts by default
    ///
    /// Default: 2
    pub excerpt_context_lines: Option<u32>,

    /// Whether to enable middle-click paste on Linux
    ///
    /// Default: true
    pub middle_click_paste: Option<bool>,

    /// What to do when multibuffer is double clicked in some of its excerpts
    /// (parts of singleton buffers).
    ///
    /// Default: select
    pub double_click_in_multibuffer: Option<DoubleClickInMultibuffer>,

    /// The minimum APCA perceptual contrast to maintain when
    /// rendering text over highlight backgrounds in the editor.
    ///
    /// Values range from 0 to 106. Set to 0 to disable adjustments.
    /// Default: 45
    #[schemars(range(min = 0, max = 106))]
    pub minimum_contrast_for_highlights: Option<MinimumContrast>,

    /// Drag and drop related settings
    pub drag_and_drop_selection: Option<DragAndDropSelectionContent>,
}

#[derive(
    Debug,
    Clone,
    Copy,
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
pub enum RelativeLineNumbers {
    Disabled,
    Enabled,
    Wrapped,
}

impl RelativeLineNumbers {
    pub fn enabled(&self) -> bool {
        match self {
            RelativeLineNumbers::Enabled | RelativeLineNumbers::Wrapped => true,
            RelativeLineNumbers::Disabled => false,
        }
    }
    pub fn wrapped(&self) -> bool {
        match self {
            RelativeLineNumbers::Enabled | RelativeLineNumbers::Disabled => false,
            RelativeLineNumbers::Wrapped => true,
        }
    }
}

// Toolbar related settings
#[with_fallible_options]
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, MergeFrom, PartialEq, Eq)]
pub struct ToolbarContent {
    /// Whether to display quick action buttons in the editor toolbar.
    ///
    /// Default: true
    pub quick_actions: Option<bool>,
    /// Whether to show the selections menu in the editor toolbar.
    ///
    /// Default: true
    pub selections_menu: Option<bool>,
}

/// Scrollbar related settings
#[with_fallible_options]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, MergeFrom, PartialEq, Default)]
pub struct ScrollbarContent {
    /// When to show the scrollbar in the editor.
    ///
    /// Default: auto
    pub show: Option<ShowScrollbar>,
    /// Whether to show buffer search result indicators in the scrollbar.
    ///
    /// Default: true
    pub search_results: Option<bool>,
    /// Whether to show selected text occurrences in the scrollbar.
    ///
    /// Default: true
    pub selected_text: Option<bool>,
    /// Whether to show cursor positions in the scrollbar.
    ///
    /// Default: true
    pub cursors: Option<bool>,
    /// Forcefully enable or disable the scrollbar for each axis
    pub axes: Option<ScrollbarAxesContent>,
}

/// Sticky scroll related settings
#[with_fallible_options]
#[derive(Clone, Default, Debug, Serialize, Deserialize, JsonSchema, MergeFrom, PartialEq)]
pub struct StickyScrollContent {
    /// Whether sticky scroll is enabled.
    ///
    /// Default: false
    pub enabled: Option<bool>,
}

/// Forcefully enable or disable the scrollbar for each axis
#[with_fallible_options]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, MergeFrom, PartialEq, Default)]
pub struct ScrollbarAxesContent {
    /// When false, forcefully disables the horizontal scrollbar. Otherwise, obey other settings.
    ///
    /// Default: true
    pub horizontal: Option<bool>,

    /// When false, forcefully disables the vertical scrollbar. Otherwise, obey other settings.
    ///
    /// Default: true
    pub vertical: Option<bool>,
}

/// Gutter related settings
#[with_fallible_options]
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, MergeFrom, PartialEq, Eq)]
pub struct GutterContent {
    /// Whether to show line numbers in the gutter.
    ///
    /// Default: true
    pub line_numbers: Option<bool>,
    /// Minimum number of characters to reserve space for in the gutter.
    ///
    /// Default: 4
    pub min_line_number_digits: Option<usize>,
    /// Whether to show fold buttons in the gutter.
    ///
    /// Default: true
    pub folds: Option<bool>,
}

#[derive(
    Copy,
    Clone,
    Debug,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    JsonSchema,
    MergeFrom,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum CurrentLineHighlight {
    // Don't highlight the current line.
    None,
    // Highlight the gutter area.
    Gutter,
    // Highlight the editor area.
    Line,
    // Highlight the full line.
    All,
}

/// What to do when multibuffer is double clicked in some of its excerpts (parts of singleton buffers).
#[derive(
    Default,
    Copy,
    Clone,
    Debug,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    JsonSchema,
    MergeFrom,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum DoubleClickInMultibuffer {
    /// Behave as a regular buffer and select the whole word.
    #[default]
    Select,
    /// Open the excerpt clicked as a new buffer in the new tab, if no `alt` modifier was pressed during double click.
    /// Otherwise, behave as a regular buffer and select the whole word.
    Open,
}

/// The key to use for adding multiple cursors
///
/// Default: alt
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
pub enum MultiCursorModifier {
    Alt,
    #[serde(alias = "cmd", alias = "ctrl")]
    CmdOrCtrl,
}

/// Whether the editor will scroll beyond the last line.
///
/// Default: one_page
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
pub enum ScrollBeyondLastLine {
    /// The editor will not scroll beyond the last line.
    Off,

    /// The editor will scroll beyond the last line by one page.
    OnePage,

    /// The editor will scroll beyond the last line by the same number of lines as vertical_scroll_margin.
    VerticalScrollMargin,
}

/// The shape of a selection cursor.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    JsonSchema,
    MergeFrom,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    /// A vertical bar
    #[default]
    Bar,
    /// A block that surrounds the following character
    Block,
    /// An underline that runs along the following character
    Underline,
    /// A box drawn around the following character
    Hollow,
}

/// Determines when the mouse cursor should be hidden in an editor or input box.
///
/// Default: on_typing_and_movement
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    JsonSchema,
    MergeFrom,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum HideMouseMode {
    /// Never hide the mouse cursor
    Never,
    /// Hide only when typing
    OnTyping,
    /// Hide on both typing and cursor movement
    #[default]
    OnTypingAndMovement,
}

/// Whether to allow drag and drop text selection in buffer.
#[with_fallible_options]
#[derive(Clone, Default, Debug, Serialize, Deserialize, JsonSchema, MergeFrom, PartialEq, Eq)]
pub struct DragAndDropSelectionContent {
    /// When true, enables drag and drop text selection in buffer.
    ///
    /// Default: true
    pub enabled: Option<bool>,

    /// The delay in milliseconds that must elapse before drag and drop is allowed. Otherwise, a new text selection is created.
    ///
    /// Default: 300
    pub delay: Option<DelayMs>,
}

/// Minimum APCA perceptual contrast for text over highlight backgrounds.
///
/// Valid range: 0.0 to 106.0
/// Default: 45.0
#[derive(
    Clone,
    Copy,
    Debug,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    PartialEq,
    PartialOrd,
    derive_more::FromStr,
)]
#[serde(transparent)]
pub struct MinimumContrast(
    #[serde(serialize_with = "crate::serialize_f32_with_two_decimal_places")] pub f32,
);

impl Display for MinimumContrast {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.1}", self.0)
    }
}

impl From<f32> for MinimumContrast {
    fn from(x: f32) -> Self {
        Self(x)
    }
}

/// Opacity of the inactive panes. 0 means transparent, 1 means opaque.
///
/// Valid range: 0.0 to 1.0
/// Default: 1.0
#[derive(
    Clone,
    Copy,
    Debug,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    PartialEq,
    PartialOrd,
    derive_more::FromStr,
)]
#[serde(transparent)]
pub struct InactiveOpacity(
    #[serde(serialize_with = "serialize_f32_with_two_decimal_places")] pub f32,
);

impl Display for InactiveOpacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.1}", self.0)
    }
}

impl From<f32> for InactiveOpacity {
    fn from(x: f32) -> Self {
        Self(x)
    }
}

/// Centered layout related setting (left/right).
///
/// Valid range: 0.0 to 0.4
/// Default: 2.0
#[derive(
    Clone,
    Copy,
    Debug,
    Serialize,
    Deserialize,
    MergeFrom,
    PartialEq,
    PartialOrd,
    derive_more::FromStr,
)]
#[serde(transparent)]
pub struct CenteredPaddingSettings(
    #[serde(serialize_with = "serialize_f32_with_two_decimal_places")] pub f32,
);

impl CenteredPaddingSettings {
    pub const MIN_PADDING: f32 = 0.0;
    // This is an f64 so serde_json can give a type hint without random numbers in the back
    pub const DEFAULT_PADDING: f64 = 0.2;
    pub const MAX_PADDING: f32 = 0.4;
}

impl Display for CenteredPaddingSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.2}", self.0)
    }
}

impl From<f32> for CenteredPaddingSettings {
    fn from(x: f32) -> Self {
        Self(x)
    }
}

impl Default for CenteredPaddingSettings {
    fn default() -> Self {
        Self(Self::DEFAULT_PADDING as f32)
    }
}

impl schemars::JsonSchema for CenteredPaddingSettings {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CenteredPaddingSettings".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        use schemars::json_schema;
        json_schema!({
            "type": "number",
            "minimum": Self::MIN_PADDING,
            "maximum": Self::MAX_PADDING,
            "default": Self::DEFAULT_PADDING,
            "description": "Centered layout related setting (left/right)."
        })
    }
}
