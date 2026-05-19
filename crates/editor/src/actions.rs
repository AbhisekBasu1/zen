//! This module contains all actions supported by [`Editor`].
use super::*;
use gpui::{Action, actions};
use schemars::JsonSchema;
use util::serde::default_true;

/// Moves the cursor to the beginning of the current line.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct MoveToBeginningOfLine {
    #[serde(default = "default_true")]
    pub stop_at_soft_wraps: bool,
    #[serde(default)]
    pub stop_at_indent: bool,
}

/// Selects from the cursor to the beginning of the current line.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct SelectToBeginningOfLine {
    #[serde(default)]
    pub(super) stop_at_soft_wraps: bool,
    #[serde(default)]
    pub stop_at_indent: bool,
}

/// Deletes from the cursor to the beginning of the current line.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct DeleteToBeginningOfLine {
    #[serde(default)]
    pub(super) stop_at_indent: bool,
}

/// Moves the cursor up by one page.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct MovePageUp {
    #[serde(default)]
    pub(super) center_cursor: bool,
}

/// Moves the cursor down by one page.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct MovePageDown {
    #[serde(default)]
    pub(super) center_cursor: bool,
}

/// Moves the cursor to the end of the current line.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct MoveToEndOfLine {
    #[serde(default = "default_true")]
    pub stop_at_soft_wraps: bool,
}

/// Selects from the cursor to the end of the current line.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct SelectToEndOfLine {
    #[serde(default)]
    pub(super) stop_at_soft_wraps: bool,
}

/// Moves the cursor up by a specified number of lines.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct MoveUpByLines {
    #[serde(default)]
    pub(super) lines: u32,
}

/// Moves the cursor down by a specified number of lines.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct MoveDownByLines {
    #[serde(default)]
    pub(super) lines: u32,
}

/// Extends selection up by a specified number of lines.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct SelectUpByLines {
    #[serde(default)]
    pub(super) lines: u32,
}

/// Extends selection down by a specified number of lines.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct SelectDownByLines {
    #[serde(default)]
    pub(super) lines: u32,
}

/// Expands all excerpts with selections.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct ExpandExcerpts {
    #[serde(default)]
    pub(super) lines: u32,
}

/// Expands excerpts above the current position.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct ExpandExcerptsUp {
    #[serde(default)]
    pub(super) lines: u32,
}

/// Expands excerpts below the current position.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct ExpandExcerptsDown {
    #[serde(default)]
    pub(super) lines: u32,
}

/// Handles text input in the editor.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
pub struct HandleInput(pub String);

/// Deletes from the cursor to the end of the next word.
/// Stops before the end of the next word, if whitespace sequences of length >= 2 are encountered.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct DeleteToNextWordEnd {
    #[serde(default)]
    pub ignore_newlines: bool,
    // Whether to stop before the end of the next word, if language-defined bracket is encountered.
    #[serde(default)]
    pub ignore_brackets: bool,
}

/// Deletes from the cursor to the start of the previous word.
/// Stops before the start of the previous word, if whitespace sequences of length >= 2 are encountered.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct DeleteToPreviousWordStart {
    #[serde(default)]
    pub ignore_newlines: bool,
    // Whether to stop before the start of the previous word, if language-defined bracket is encountered.
    #[serde(default)]
    pub ignore_brackets: bool,
}

/// Deletes from the cursor to the end of the next subword.
/// Stops before the end of the next subword, if whitespace sequences of length >= 2 are encountered.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct DeleteToNextSubwordEnd {
    #[serde(default)]
    pub ignore_newlines: bool,
    // Whether to stop before the start of the previous word, if language-defined bracket is encountered.
    #[serde(default)]
    pub ignore_brackets: bool,
}

/// Deletes from the cursor to the start of the previous subword.
/// Stops before the start of the previous subword, if whitespace sequences of length >= 2 are encountered.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct DeleteToPreviousSubwordStart {
    #[serde(default)]
    pub ignore_newlines: bool,
    // Whether to stop before the start of the previous word, if language-defined bracket is encountered.
    #[serde(default)]
    pub ignore_brackets: bool,
}

/// Cuts from cursor to end of line.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct CutToEndOfLine {
    #[serde(default)]
    pub stop_at_newlines: bool,
}

/// Folds all code blocks at the specified indentation level.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
pub struct FoldAtLevel(pub u32);

/// Splits selection into individual lines.
#[derive(PartialEq, Clone, Deserialize, Default, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct SplitSelectionIntoLines {
    /// Keep the text selected after splitting instead of collapsing to cursors.
    #[serde(default)]
    pub keep_selections: bool,
}

/// Adds a cursor above the current selection.
#[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct AddSelectionAbove {
    #[serde(default = "default_true")]
    pub skip_soft_wrap: bool,
}

/// Adds a cursor below the current selection.
#[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = editor)]
#[serde(deny_unknown_fields)]
pub struct AddSelectionBelow {
    #[serde(default = "default_true")]
    pub skip_soft_wrap: bool,
}

actions!(
    go_to_line,
    [
        /// Toggles the go to line dialog.
        #[action(name = "Toggle")]
        ToggleGoToLine
    ]
);

actions!(
    editor,
    [
        /// Deletes the character before the cursor.
        Backspace,
        /// Cancels the current operation.
        Cancel,
        /// Confirms the rename operation.
        /// Copies selected text to the clipboard.
        Copy,
        /// Copies selected text to the clipboard with leading/trailing whitespace trimmed.
        CopyAndTrim,
        /// Copies the current file location to the clipboard.
        CopyFileLocation,
        /// Copies the highlighted text as JSON.
        CopyHighlightJson,
        /// Copies the current file name to the clipboard.
        CopyFileName,
        /// Copies the file name without extension to the clipboard.
        CopyFileNameWithoutExtension,
        /// Cuts selected text to the clipboard.
        Cut,
        /// Deletes the character after the cursor.
        Delete,
        /// Deletes the current line.
        DeleteLine,
        /// Deletes from cursor to end of line.
        DeleteToEndOfLine,
        /// Folds the current code block.
        Fold,
        /// Folds all foldable regions in the editor.
        FoldAll,
        /// Folds all code blocks at indentation level 1.
        #[action(name = "FoldAtLevel_1")]
        FoldAtLevel1,
        /// Folds all code blocks at indentation level 2.
        #[action(name = "FoldAtLevel_2")]
        FoldAtLevel2,
        /// Folds all code blocks at indentation level 3.
        #[action(name = "FoldAtLevel_3")]
        FoldAtLevel3,
        /// Folds all code blocks at indentation level 4.
        #[action(name = "FoldAtLevel_4")]
        FoldAtLevel4,
        /// Folds all code blocks at indentation level 5.
        #[action(name = "FoldAtLevel_5")]
        FoldAtLevel5,
        /// Folds all code blocks at indentation level 6.
        #[action(name = "FoldAtLevel_6")]
        FoldAtLevel6,
        /// Folds all code blocks at indentation level 7.
        #[action(name = "FoldAtLevel_7")]
        FoldAtLevel7,
        /// Folds all code blocks at indentation level 8.
        #[action(name = "FoldAtLevel_8")]
        FoldAtLevel8,
        /// Folds all code blocks at indentation level 9.
        #[action(name = "FoldAtLevel_9")]
        FoldAtLevel9,
        /// Folds all function bodies in the editor.
        FoldFunctionBodies,
        /// Folds the current code block and all its children.
        FoldRecursive,
        /// Folds the selected ranges.
        FoldSelectedRanges,
        /// Toggles focus back to the last active buffer.
        ToggleFocus,
        /// Toggles folding at the current position.
        ToggleFold,
        /// Toggles recursive folding at the current position.
        ToggleFoldRecursive,
        /// Toggles all folds in a buffer or all excerpts in multibuffer.
        ToggleFoldAll,
        /// Scrolls down by half a page.
        HalfPageDown,
        /// Scrolls up by half a page.
        HalfPageUp,
        /// Increases indentation of selected lines.
        Indent,
        /// Joins the current line with the next line.
        JoinLines,
        /// Cuts to kill ring (Emacs-style).
        KillRingCut,
        /// Yanks from kill ring (Emacs-style).
        KillRingYank,
        /// Moves cursor down one line.
        LineDown,
        /// Moves cursor up one line.
        LineUp,
        /// Moves cursor left.
        MoveLeft,
        /// Moves cursor right.
        MoveRight,
        /// Moves cursor to the beginning of the document.
        MoveToBeginning,
        /// Moves cursor to the end of the document.
        MoveToEnd,
        /// Moves cursor to the end of the paragraph.
        MoveToEndOfParagraph,
        /// Moves cursor to the end of the next subword.
        MoveToNextSubwordEnd,
        /// Moves cursor to the end of the next word.
        MoveToNextWordEnd,
        /// Moves cursor to the start of the previous subword.
        MoveToPreviousSubwordStart,
        /// Moves cursor to the start of the previous word.
        MoveToPreviousWordStart,
        /// Moves cursor to the start of the paragraph.
        MoveToStartOfParagraph,
        /// Moves cursor to the start of the current excerpt.
        MoveToStartOfExcerpt,
        /// Moves cursor to the start of the next excerpt.
        MoveToStartOfNextExcerpt,
        /// Moves cursor to the end of the current excerpt.
        MoveToEndOfExcerpt,
        /// Moves cursor to the end of the previous excerpt.
        MoveToEndOfPreviousExcerpt,
        /// Inserts a new line and moves cursor to it.
        Newline,
        /// Inserts a new line above the current line.
        NewlineAbove,
        /// Inserts a new line below the current line.
        NewlineBelow,
        /// Scrolls to the next screen.
        NextScreen,
        /// Opens the context menu at cursor position.
        OpenContextMenu,
        /// Opens excerpts from the current file.
        OpenExcerpts,
        /// Opens excerpts in a split pane.
        OpenExcerptsSplit,
        /// Decreases indentation of selected lines.
        Outdent,
        /// Automatically adjusts indentation based on context.
        AutoIndent,
        /// Scrolls down by one page.
        PageDown,
        /// Scrolls up by one page.
        PageUp,
        /// Pastes from clipboard.
        Paste,
        /// Redoes the last undone edit.
        Redo,
        /// Reloads the file from disk.
        ReloadFile,
        /// Scrolls the cursor to the bottom of the viewport.
        ScrollCursorBottom,
        /// Scrolls the cursor to the center of the viewport.
        ScrollCursorCenter,
        /// Cycles cursor position between center, top, and bottom.
        ScrollCursorCenterTopBottom,
        /// Scrolls the cursor to the top of the viewport.
        ScrollCursorTop,
        /// Selects all text in the editor.
        SelectAll,
        /// Selects to the start of the current excerpt.
        SelectToStartOfExcerpt,
        /// Selects to the start of the next excerpt.
        SelectToStartOfNextExcerpt,
        /// Selects to the end of the current excerpt.
        SelectToEndOfExcerpt,
        /// Selects to the end of the previous excerpt.
        SelectToEndOfPreviousExcerpt,
        /// Extends selection down.
        SelectDown,
        /// Extends selection left.
        SelectLeft,
        /// Selects the current line.
        SelectLine,
        /// Extends selection down by one page.
        SelectPageDown,
        /// Extends selection up by one page.
        SelectPageUp,
        /// Extends selection right.
        SelectRight,
        /// Selects to the beginning of the document.
        SelectToBeginning,
        /// Selects to the end of the document.
        SelectToEnd,
        /// Selects to the end of the paragraph.
        SelectToEndOfParagraph,
        /// Selects to the end of the next subword.
        SelectToNextSubwordEnd,
        /// Selects to the end of the next word.
        SelectToNextWordEnd,
        /// Selects to the start of the previous subword.
        SelectToPreviousSubwordStart,
        /// Selects to the start of the previous word.
        SelectToPreviousWordStart,
        /// Selects to the start of the paragraph.
        SelectToStartOfParagraph,
        /// Extends selection up.
        SelectUp,
        /// Shows the system character palette.
        ShowCharacterPalette,
        /// Inserts a tab character or indents.
        Tab,
        /// Removes a tab character or outdents.
        Backtab,
        /// Toggles indent guides display.
        ToggleIndentGuides,
        /// Toggles line numbers display.
        ToggleLineNumbers,
        /// Swaps the start and end of the current selection.
        SwapSelectionEnds,
        /// Sets a mark at the current position.
        SetMark,
        /// Toggles relative line numbers display.
        ToggleRelativeLineNumbers,
        /// Toggles soft wrap mode.
        ToggleSoftWrap,
        /// Toggles the tab bar display.
        ToggleTabBar,
        /// Undoes the last edit.
        Undo,
        /// Unfolds all folded regions.
        UnfoldAll,
        /// Unfolds lines at cursor.
        UnfoldLines,
        /// Unfolds recursively at cursor.
        UnfoldRecursive,
        /// Wraps selections in tag specified by language.
        WrapSelectionsInTag,
        /// Saves the current location to navigation history.
        SaveLocation,
    ]
);
