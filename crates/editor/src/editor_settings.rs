use gpui::App;
use language::CursorShape;
pub use settings::{
    CurrentLineHighlight, DelayMs, DoubleClickInMultibuffer, HideMouseMode, MultiCursorModifier,
    ScrollBeyondLastLine,
};
use settings::{RegisterSetting, RelativeLineNumbers, Settings};
use ui::scrollbars::ShowScrollbar;

/// Imports from the VSCode settings at
/// https://code.visualstudio.com/docs/reference/default-settings
#[derive(Clone, RegisterSetting)]
pub struct EditorSettings {
    pub cursor_blink: bool,
    pub cursor_shape: Option<CursorShape>,
    pub current_line_highlight: CurrentLineHighlight,
    pub selection_highlight: bool,
    pub rounded_selection: bool,
    pub toolbar: Toolbar,
    pub scrollbar: Scrollbar,
    pub gutter: Gutter,
    pub scroll_beyond_last_line: ScrollBeyondLastLine,
    pub vertical_scroll_margin: f64,
    pub autoscroll_on_clicks: bool,
    pub horizontal_scroll_margin: f32,
    pub scroll_sensitivity: f32,
    pub mouse_wheel_zoom: bool,
    pub fast_scroll_sensitivity: f32,
    pub sticky_scroll: StickyScroll,
    pub relative_line_numbers: RelativeLineNumbers,
    pub multi_cursor_modifier: MultiCursorModifier,
    pub redact_private_values: bool,
    pub expand_excerpt_lines: u32,
    pub excerpt_context_lines: u32,
    pub middle_click_paste: bool,
    pub double_click_in_multibuffer: DoubleClickInMultibuffer,
    pub hide_mouse: Option<HideMouseMode>,
    pub drag_and_drop_selection: DragAndDropSelection,
    pub minimum_contrast_for_highlights: f32,
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StickyScroll {
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolbar {
    pub quick_actions: bool,
    pub selections_menu: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Scrollbar {
    pub show: ShowScrollbar,
    pub selected_text: bool,
    pub search_results: bool,
    pub cursors: bool,
    pub axes: ScrollbarAxes,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Gutter {
    pub min_line_number_digits: usize,
    pub line_numbers: bool,
    pub folds: bool,
}

/// Forcefully enable or disable the scrollbar for each axis
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ScrollbarAxes {
    /// When false, forcefully disables the horizontal scrollbar. Otherwise, obey other settings.
    ///
    /// Default: true
    pub horizontal: bool,

    /// When false, forcefully disables the vertical scrollbar. Otherwise, obey other settings.
    ///
    /// Default: true
    pub vertical: bool,
}

/// Whether to allow drag and drop text selection in buffer.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub struct DragAndDropSelection {
    /// When true, enables drag and drop text selection in buffer.
    ///
    /// Default: true
    pub enabled: bool,

    /// The delay in milliseconds that must elapse before drag and drop is allowed. Otherwise, a new text selection is created.
    ///
    /// Default: 300
    pub delay: DelayMs,
}

impl Settings for EditorSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let editor = content.editor.clone();
        let scrollbar = editor.scrollbar.unwrap();
        let gutter = editor.gutter.unwrap();
        let axes = scrollbar.axes.unwrap();
        let toolbar = editor.toolbar.unwrap();
        let drag_and_drop_selection = editor.drag_and_drop_selection.unwrap();
        let sticky_scroll = editor.sticky_scroll.unwrap();
        Self {
            cursor_blink: editor.cursor_blink.unwrap(),
            cursor_shape: editor.cursor_shape.map(Into::into),
            current_line_highlight: editor.current_line_highlight.unwrap(),
            selection_highlight: editor.selection_highlight.unwrap(),
            rounded_selection: editor.rounded_selection.unwrap(),
            toolbar: Toolbar {
                quick_actions: toolbar.quick_actions.unwrap(),
                selections_menu: toolbar.selections_menu.unwrap(),
            },
            scrollbar: Scrollbar {
                show: scrollbar.show.map(ui_scrollbar_settings_from_raw).unwrap(),
                selected_text: scrollbar.selected_text.unwrap(),
                search_results: scrollbar.search_results.unwrap(),
                cursors: scrollbar.cursors.unwrap(),
                axes: ScrollbarAxes {
                    horizontal: axes.horizontal.unwrap(),
                    vertical: axes.vertical.unwrap(),
                },
            },
            gutter: Gutter {
                min_line_number_digits: gutter.min_line_number_digits.unwrap(),
                line_numbers: gutter.line_numbers.unwrap(),
                folds: gutter.folds.unwrap(),
            },
            scroll_beyond_last_line: editor.scroll_beyond_last_line.unwrap(),
            vertical_scroll_margin: editor.vertical_scroll_margin.unwrap() as f64,
            autoscroll_on_clicks: editor.autoscroll_on_clicks.unwrap(),
            horizontal_scroll_margin: editor.horizontal_scroll_margin.unwrap(),
            scroll_sensitivity: editor.scroll_sensitivity.unwrap(),
            mouse_wheel_zoom: editor.mouse_wheel_zoom.unwrap(),
            fast_scroll_sensitivity: editor.fast_scroll_sensitivity.unwrap(),
            sticky_scroll: StickyScroll {
                enabled: sticky_scroll.enabled.unwrap(),
            },
            relative_line_numbers: editor.relative_line_numbers.unwrap(),
            multi_cursor_modifier: editor.multi_cursor_modifier.unwrap(),
            redact_private_values: editor.redact_private_values.unwrap(),
            expand_excerpt_lines: editor.expand_excerpt_lines.unwrap(),
            excerpt_context_lines: editor.excerpt_context_lines.unwrap(),
            middle_click_paste: editor.middle_click_paste.unwrap(),
            double_click_in_multibuffer: editor.double_click_in_multibuffer.unwrap(),
            hide_mouse: editor.hide_mouse,
            drag_and_drop_selection: DragAndDropSelection {
                enabled: drag_and_drop_selection.enabled.unwrap(),
                delay: drag_and_drop_selection.delay.unwrap(),
            },
            minimum_contrast_for_highlights: editor.minimum_contrast_for_highlights.unwrap().0,
        }
    }
}

#[derive(Default)]
pub struct EditorSettingsScrollbarProxy;

impl ui::scrollbars::ScrollbarVisibility for EditorSettingsScrollbarProxy {
    fn visibility(&self, cx: &App) -> ShowScrollbar {
        EditorSettings::get_global(cx).scrollbar.show
    }
}

pub fn ui_scrollbar_settings_from_raw(
    value: settings::ShowScrollbar,
) -> ui::scrollbars::ShowScrollbar {
    match value {
        settings::ShowScrollbar::Auto => ShowScrollbar::Auto,
        settings::ShowScrollbar::System => ShowScrollbar::System,
        settings::ShowScrollbar::Always => ShowScrollbar::Always,
        settings::ShowScrollbar::Never => ShowScrollbar::Never,
    }
}
