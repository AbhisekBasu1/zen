use std::num::NonZeroU32;

use collections::HashMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};
use std::sync::Arc;

use crate::{ExtendingVec, merge_from};

#[with_fallible_options]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AllLanguageSettingsContent {
    /// The default language settings.
    #[serde(flatten)]
    pub defaults: LanguageSettingsContent,
    /// The settings for individual languages.
    #[serde(default)]
    pub languages: LanguageToSettingsMap,
    /// Settings for associating file extensions and filenames
    /// with languages.
    pub file_types: Option<HashMap<Arc<str>, ExtendingVec<String>>>,
}

impl merge_from::MergeFrom for AllLanguageSettingsContent {
    fn merge_from(&mut self, other: &Self) {
        self.file_types.merge_from(&other.file_types);

        // A user's global settings override the default global settings and
        // all default language-specific settings.
        //
        self.defaults.merge_from(&other.defaults);
        for language_settings in self.languages.0.values_mut() {
            language_settings.merge_from(&other.defaults);
        }

        // A user's language-specific settings override default language-specific settings.
        for (language_name, user_language_settings) in &other.languages.0 {
            if let Some(existing) = self.languages.0.get_mut(language_name) {
                existing.merge_from(&user_language_settings);
            } else {
                let mut new_settings = self.defaults.clone();
                new_settings.merge_from(&user_language_settings);

                self.languages.0.insert(language_name.clone(), new_settings);
            }
        }
    }
}

/// Controls the soft-wrapping behavior in the editor.
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
pub enum AutoIndentMode {
    /// Adjusts indentation based on syntax context when typing.
    /// Uses tree-sitter to analyze code structure and indent accordingly.
    SyntaxAware,
    /// Preserve the indentation of the current line when creating new lines,
    /// but don't adjust based on syntax context.
    PreserveIndent,
    /// No automatic indentation. New lines start at column 0.
    None,
}

/// Controls the soft-wrapping behavior in the editor.
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
pub enum SoftWrap {
    /// Prefer a single line generally, unless an overly long line is encountered.
    None,
    /// Deprecated: use None instead. Left to avoid breaking existing users' configs.
    /// Prefer a single line generally, unless an overly long line is encountered.
    PreferLine,
    /// Soft wrap lines that exceed the editor width.
    EditorWidth,
    /// Soft wrap line at the preferred line length or the editor width (whichever is smaller).
    #[serde(alias = "preferred_line_length")]
    Bounded,
}

/// The settings for a particular language.
#[with_fallible_options]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct LanguageSettingsContent {
    /// How many columns a tab should occupy.
    ///
    /// Default: 4
    #[schemars(range(min = 1, max = 128))]
    pub tab_size: Option<NonZeroU32>,
    /// Whether to indent lines using tab characters, as opposed to multiple
    /// spaces.
    ///
    /// Default: false
    pub hard_tabs: Option<bool>,
    /// How to soft-wrap long lines of text.
    ///
    /// Default: none
    pub soft_wrap: Option<SoftWrap>,
    /// The column at which to soft-wrap lines, for buffers where soft-wrap
    /// is enabled.
    ///
    /// Default: 80
    pub preferred_line_length: Option<u32>,
    /// Whether to show wrap guides in the editor. Setting this to true will
    /// show a guide at the 'preferred_line_length' value if softwrap is set to
    /// 'preferred_line_length', and will show any additional guides as specified
    /// by the 'wrap_guides' setting.
    ///
    /// Default: true
    pub show_wrap_guides: Option<bool>,
    /// Character counts at which to show wrap guides in the editor.
    ///
    /// Default: []
    pub wrap_guides: Option<Vec<usize>>,
    /// Indent guide related settings.
    pub indent_guides: Option<IndentGuideSettingsContent>,
    /// Whether or not to remove any trailing whitespace from lines of a buffer
    /// before saving it.
    ///
    /// Default: true
    pub remove_trailing_whitespace_on_save: Option<bool>,
    /// Whether or not to ensure there's a single newline at the end of a buffer
    /// when saving it.
    ///
    /// Default: true
    pub ensure_final_newline_on_save: Option<bool>,
    /// How line endings should be handled for new files and during format and
    /// save operations.
    ///
    /// - `detect`: Detect existing line endings and otherwise use the platform
    ///   default (`lf` on Unix, `crlf` on Windows).
    /// - `prefer_lf`: Prefer LF for new files and files with no existing line
    ///   ending.
    /// - `prefer_crlf`: Prefer CRLF for new files and files with no existing
    ///   line ending.
    /// - `enforce_lf`: Enforce LF during format and save.
    /// - `enforce_crlf`: Enforce CRLF during format and save.
    ///
    /// The EditorConfig `end_of_line` property overrides this setting and
    /// behaves like `enforce_lf` or `enforce_crlf`.
    ///
    /// Default: detect
    pub line_ending: Option<LineEndingSetting>,
    /// Controls where the `editor::Rewrap` action is allowed for this language.
    ///
    /// Default: "in_comments"
    pub allow_rewrap: Option<RewrapBehavior>,
    /// Whether to show tabs and spaces in the editor.
    pub show_whitespaces: Option<ShowWhitespaceSetting>,
    /// Visible characters used to render whitespace when show_whitespaces is enabled.
    ///
    /// Default: "•" for spaces, "→" for tabs.
    pub whitespace_map: Option<WhitespaceMapContent>,
    /// Whether to start a new line with a comment when a previous line is a comment as well.
    ///
    /// Default: true
    pub extend_comment_on_newline: Option<bool>,
    /// Whether to continue markdown lists when pressing enter.
    ///
    /// Default: true
    pub extend_list_on_newline: Option<bool>,
    /// Whether to indent list items when pressing tab after a list marker.
    ///
    /// Default: true
    pub indent_list_on_tab: Option<bool>,
    /// Whether to automatically type closing characters for you. For example,
    /// when you type '(', Zed will automatically add a closing ')' at the correct position.
    ///
    /// Default: true
    pub use_autoclose: Option<bool>,
    /// Whether to automatically surround text with characters for you. For example,
    /// when you select text and type '(', Zed will automatically surround text with ().
    ///
    /// Default: true
    pub use_auto_surround: Option<bool>,
    /// Controls how the editor handles the autoclosed characters.
    /// When set to `false`(default), skipping over and auto-removing of the closing characters
    /// happen only for auto-inserted characters.
    /// Otherwise(when `true`), the closing characters are always skipped over and auto-removed
    /// no matter how they were inserted.
    ///
    /// Default: false
    pub always_treat_brackets_as_autoclosed: Option<bool>,
    /// Controls automatic indentation behavior when typing.
    ///
    /// - "syntax_aware": Adjusts indentation based on syntax context (default)
    /// - "preserve_indent": Preserves current line's indentation on new lines
    /// - "none": No automatic indentation
    ///
    /// Default: syntax_aware
    pub auto_indent: Option<AutoIndentMode>,
    /// Whether indentation of pasted content should be adjusted based on the context.
    ///
    /// Default: true
    pub auto_indent_on_paste: Option<bool>,
    /// Whether to enable word diff highlighting in the editor.
    ///
    /// When enabled, changed words within modified lines are highlighted
    /// to show exactly what changed.
    ///
    /// Default: true
    pub word_diff_enabled: Option<bool>,
    /// Whether to use tree-sitter bracket queries to detect and colorize the brackets in the editor.
    ///
    /// Default: false
    pub colorize_brackets: Option<bool>,
}

/// Controls how whitespace should be displayedin the editor.
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
pub enum ShowWhitespaceSetting {
    /// Draw whitespace only for the selected text.
    Selection,
    /// Do not draw any tabs or spaces.
    None,
    /// Draw all invisible symbols.
    All,
    /// Draw whitespaces at boundaries only.
    ///
    /// For a whitespace to be on a boundary, any of the following conditions need to be met:
    /// - It is a tab
    /// - It is adjacent to an edge (start or end)
    /// - It is adjacent to a whitespace (left or right)
    Boundary,
    /// Draw whitespaces only after non-whitespace characters.
    Trailing,
}

#[with_fallible_options]
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, MergeFrom, PartialEq)]
pub struct WhitespaceMapContent {
    pub space: Option<char>,
    pub tab: Option<char>,
}

/// The behavior of `editor::Rewrap`.
#[derive(
    Debug,
    PartialEq,
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
pub enum RewrapBehavior {
    /// Only rewrap within comments.
    #[default]
    InComments,
    /// Only rewrap within the current selection(s).
    InSelections,
    /// Allow rewrapping anywhere.
    Anywhere,
}

/// Controls how line endings are normalized when a buffer is saved.
#[derive(
    Debug,
    Clone,
    Copy,
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
pub enum LineEndingSetting {
    /// Preserve the existing line endings of the file. New files use the
    /// platform default line ending.
    #[strum(serialize = "Detect")]
    Detect,
    /// Use LF for new files and files with no existing line-ending
    /// convention, while preserving existing LF or CRLF files.
    #[strum(serialize = "Prefer LF")]
    PreferLf,
    /// Use CRLF for new files and files with no existing line-ending
    /// convention, while preserving existing LF or CRLF files.
    #[strum(serialize = "Prefer CRLF")]
    PreferCrlf,
    /// Normalize line endings to LF (`\n`) during format and save.
    #[strum(serialize = "Enforce LF")]
    EnforceLf,
    /// Normalize line endings to CRLF (`\r\n`) during format and save.
    #[strum(serialize = "Enforce CRLF")]
    EnforceCrlf,
}

/// The settings for indent guides.
#[with_fallible_options]
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct IndentGuideSettingsContent {
    /// Whether to display indent guides in the editor.
    ///
    /// Default: true
    pub enabled: Option<bool>,
    /// The width of the indent guides in pixels, between 1 and 10.
    ///
    /// Default: 1
    pub line_width: Option<u32>,
    /// The width of the active indent guide in pixels, between 1 and 10.
    ///
    /// Default: 1
    pub active_line_width: Option<u32>,
    /// Determines how indent guides are colored.
    ///
    /// Default: Fixed
    pub coloring: Option<IndentGuideColoring>,
    /// Determines how indent guide backgrounds are colored.
    ///
    /// Default: Disabled
    pub background_coloring: Option<IndentGuideBackgroundColoring>,
}

/// Map from language name to settings.
#[with_fallible_options]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct LanguageToSettingsMap(pub HashMap<String, LanguageSettingsContent>);

/// Determines how indent guides are colored.
#[derive(
    Default,
    Debug,
    Copy,
    Clone,
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
pub enum IndentGuideColoring {
    /// Do not render any lines for indent guides.
    Disabled,
    /// Use the same color for all indentation levels.
    #[default]
    Fixed,
    /// Use a different color for each indentation level.
    IndentAware,
}

/// Determines how indent guide backgrounds are colored.
#[derive(
    Default,
    Debug,
    Copy,
    Clone,
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
pub enum IndentGuideBackgroundColoring {
    /// Do not render any background for indent guides.
    #[default]
    Disabled,
    /// Use a different color for each indentation level.
    IndentAware,
}
