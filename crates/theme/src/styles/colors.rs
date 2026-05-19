#![allow(missing_docs)]

use gpui::{App, Hsla, SharedString, WindowBackgroundAppearance};
use refineable::Refineable;
use std::sync::Arc;
use strum::{AsRefStr, EnumIter, IntoEnumIterator};

use crate::{
    AccentColors, ActiveTheme, PlayerColors, StatusColors, StatusColorsRefinement, SyntaxTheme,
    SystemColors,
};

#[derive(Refineable, Clone, Debug, PartialEq)]
#[refineable(Debug, serde::Deserialize)]
pub struct ThemeColors {
    /// Border color. Used for most borders, is usually a high contrast color.
    pub border: Hsla,
    /// Border color. Used for deemphasized borders, like a visual divider between two sections
    pub border_variant: Hsla,
    /// Border color. Used for focused elements, like keyboard focused list item.
    pub border_focused: Hsla,
    /// Border color. Used for selected elements, like an active search filter or selected checkbox.
    pub border_selected: Hsla,
    /// Border color. Used for transparent borders. Used for placeholder borders when an element gains a border on state change.
    pub border_transparent: Hsla,
    /// Border color. Used for disabled elements, like a disabled input or button.
    pub border_disabled: Hsla,
    /// Border color. Used for elevated surfaces, like a context menu, popup, or dialog.
    pub elevated_surface_background: Hsla,
    /// Background Color. Used for grounded surfaces like a panel or tab.
    pub surface_background: Hsla,
    /// Background Color. Used for the app background and blank panels or windows.
    pub background: Hsla,
    /// Background Color. Used for the background of an element that should have a different background than the surface it's on.
    ///
    /// Elements might include: Buttons, Inputs, Checkboxes, Radio Buttons...
    ///
    /// For an element that should have the same background as the surface it's on, use `ghost_element_background`.
    pub element_background: Hsla,
    /// Background Color. Used for the hover state of an element that should have a different background than the surface it's on.
    ///
    /// Hover states are triggered by the mouse entering an element, or a finger touching an element on a touch screen.
    pub element_hover: Hsla,
    /// Background Color. Used for the active state of an element that should have a different background than the surface it's on.
    ///
    /// Active states are triggered by the mouse button being pressed down on an element, or the Return button or other activator being pressed.
    pub element_active: Hsla,
    /// Background Color. Used for the selected state of an element that should have a different background than the surface it's on.
    ///
    /// Selected states are triggered by the element being selected (or "activated") by the user.
    ///
    /// This could include a selected checkbox, a toggleable button that is toggled on, etc.
    pub element_selected: Hsla,
    /// Background Color. Used for the background of selections in a UI element.
    pub element_selection_background: Hsla,
    /// Background Color. Used for the disabled state of an element that should have a different background than the surface it's on.
    ///
    /// Disabled states are shown when a user cannot interact with an element, like a disabled button or input.
    pub element_disabled: Hsla,
    /// Background Color. Used for the area that shows where a dragged element will be dropped.
    pub drop_target_background: Hsla,
    /// Border Color. Used for the border that shows where a dragged element will be dropped.
    pub drop_target_border: Hsla,
    /// Used for the background of a ghost element that should have the same background as the surface it's on.
    ///
    /// Elements might include: Buttons, Inputs, Checkboxes, Radio Buttons...
    ///
    /// For an element that should have a different background than the surface it's on, use `element_background`.
    pub ghost_element_background: Hsla,
    /// Background Color. Used for the hover state of a ghost element that should have the same background as the surface it's on.
    ///
    /// Hover states are triggered by the mouse entering an element, or a finger touching an element on a touch screen.
    pub ghost_element_hover: Hsla,
    /// Background Color. Used for the active state of a ghost element that should have the same background as the surface it's on.
    ///
    /// Active states are triggered by the mouse button being pressed down on an element, or the Return button or other activator being pressed.
    pub ghost_element_active: Hsla,
    /// Background Color. Used for the selected state of a ghost element that should have the same background as the surface it's on.
    ///
    /// Selected states are triggered by the element being selected (or "activated") by the user.
    ///
    /// This could include a selected checkbox, a toggleable button that is toggled on, etc.
    pub ghost_element_selected: Hsla,
    /// Background Color. Used for the disabled state of a ghost element that should have the same background as the surface it's on.
    ///
    /// Disabled states are shown when a user cannot interact with an element, like a disabled button or input.
    pub ghost_element_disabled: Hsla,
    /// Text Color. Default text color used for most text.
    pub text: Hsla,
    /// Text Color. Color of muted or deemphasized text. It is a subdued version of the standard text color.
    pub text_muted: Hsla,
    /// Text Color. Color of the placeholder text typically shown in input fields to guide the user to enter valid data.
    pub text_placeholder: Hsla,
    /// Text Color. Color used for text denoting disabled elements. Typically, the color is faded or grayed out to emphasize the disabled state.
    pub text_disabled: Hsla,
    /// Text Color. Color used for emphasis or highlighting certain text, like an active filter or a matched character in a search.
    pub text_accent: Hsla,
    /// Fill Color. Used for the default fill color of an icon.
    pub icon: Hsla,
    /// Fill Color. Used for the muted or deemphasized fill color of an icon.
    ///
    /// This might be used to show an icon in an inactive pane, or to deemphasize a series of icons to give them less visual weight.
    pub icon_muted: Hsla,
    /// Fill Color. Used for the disabled fill color of an icon.
    ///
    /// Disabled states are shown when a user cannot interact with an element, like a icon button.
    pub icon_disabled: Hsla,
    /// Fill Color. Used for the placeholder fill color of an icon.
    ///
    /// This might be used to show an icon in an input that disappears when the user enters text.
    pub icon_placeholder: Hsla,
    /// Fill Color. Used for the accent fill color of an icon.
    ///
    /// This might be used to show when a toggleable icon button is selected.
    pub icon_accent: Hsla,

    // ===
    // UI Elements
    // ===
    pub status_bar_background: Hsla,
    pub title_bar_background: Hsla,
    pub title_bar_inactive_background: Hsla,
    pub toolbar_background: Hsla,
    pub tab_bar_background: Hsla,
    pub tab_inactive_background: Hsla,
    pub tab_active_background: Hsla,
    pub search_match_background: Hsla,
    pub search_active_match_background: Hsla,
    pub panel_background: Hsla,
    pub panel_focused_border: Hsla,
    pub panel_indent_guide: Hsla,
    pub panel_indent_guide_hover: Hsla,
    pub panel_indent_guide_active: Hsla,

    /// The color of the overlay surface on top of panel.
    pub panel_overlay_background: Hsla,
    /// The color of the overlay surface on top of panel when hovered over.
    pub panel_overlay_hover: Hsla,

    pub pane_focused_border: Hsla,
    pub pane_group_border: Hsla,
    /// The color of the scrollbar thumb.
    pub scrollbar_thumb_background: Hsla,
    /// The color of the scrollbar thumb when hovered over.
    pub scrollbar_thumb_hover_background: Hsla,
    /// The color of the scrollbar thumb whilst being actively dragged.
    pub scrollbar_thumb_active_background: Hsla,
    /// The border color of the scrollbar thumb.
    pub scrollbar_thumb_border: Hsla,
    /// The background color of the scrollbar track.
    pub scrollbar_track_background: Hsla,
    /// The border color of the scrollbar track.
    pub scrollbar_track_border: Hsla,
    // ===
    // Editor
    // ===
    pub editor_foreground: Hsla,
    pub editor_background: Hsla,
    pub editor_gutter_background: Hsla,
    pub editor_subheader_background: Hsla,
    pub editor_active_line_background: Hsla,
    pub editor_highlighted_line_background: Hsla,
    /// Text Color. Used for the text of the line number in the editor gutter.
    pub editor_line_number: Hsla,
    /// Text Color. Used for the text of the line number in the editor gutter when the line is highlighted.
    pub editor_active_line_number: Hsla,
    /// Text Color. Used for the text of the line number in the editor gutter when the line is hovered over.
    pub editor_hover_line_number: Hsla,
    /// Text Color. Used to mark invisible characters in the editor.
    ///
    /// Example: spaces, tabs, carriage returns, etc.
    pub editor_invisible: Hsla,
    pub editor_wrap_guide: Hsla,
    pub editor_active_wrap_guide: Hsla,
    pub editor_indent_guide: Hsla,
    pub editor_indent_guide_active: Hsla,
    /// Read-access of a symbol, like reading a variable.
    ///
    /// A document highlight is a range inside a text document which deserves
    /// special attention. Usually a document highlight is visualized by changing
    /// the background color of its range.
    pub editor_document_highlight_read_background: Hsla,
    /// Read-access of a symbol, like reading a variable.
    ///
    /// A document highlight is a range inside a text document which deserves
    /// special attention. Usually a document highlight is visualized by changing
    /// the background color of its range.
    pub editor_document_highlight_write_background: Hsla,
    /// Highlighted brackets background color.
    ///
    /// Matching brackets in the cursor scope are highlighted with this background color.
    pub editor_document_highlight_bracket_background: Hsla,

    /// Represents a link text hover color.
    pub link_text_hover: Hsla,

    /// Represents an added entry or hunk in vcs, like git.
    pub version_control_added: Hsla,
    /// Represents a deleted entry in version control systems.
    pub version_control_deleted: Hsla,
    /// Represents a modified entry in version control systems.
    pub version_control_modified: Hsla,
    /// Represents a renamed entry in version control systems.
    pub version_control_renamed: Hsla,
    /// Represents a conflicting entry in version control systems.
    pub version_control_conflict: Hsla,
    /// Represents an ignored entry in version control systems.
    pub version_control_ignored: Hsla,
    /// Represents an added word in a word diff.
    pub version_control_word_added: Hsla,
    /// Represents a deleted word in a word diff.
    pub version_control_word_deleted: Hsla,
    /// Represents the "ours" region of a merge conflict.
    pub version_control_conflict_marker_ours: Hsla,
    /// Represents the "theirs" region of a merge conflict.
    pub version_control_conflict_marker_theirs: Hsla,
}

#[derive(EnumIter, Debug, Clone, Copy, AsRefStr)]
#[strum(serialize_all = "snake_case")]
pub enum ThemeColorField {
    Border,
    BorderVariant,
    BorderFocused,
    BorderSelected,
    BorderTransparent,
    BorderDisabled,
    ElevatedSurfaceBackground,
    SurfaceBackground,
    Background,
    ElementBackground,
    ElementHover,
    ElementActive,
    ElementSelected,
    ElementDisabled,
    DropTargetBackground,
    DropTargetBorder,
    GhostElementBackground,
    GhostElementHover,
    GhostElementActive,
    GhostElementSelected,
    GhostElementDisabled,
    Text,
    TextMuted,
    TextPlaceholder,
    TextDisabled,
    TextAccent,
    Icon,
    IconMuted,
    IconDisabled,
    IconPlaceholder,
    IconAccent,
    StatusBarBackground,
    TitleBarBackground,
    TitleBarInactiveBackground,
    ToolbarBackground,
    TabBarBackground,
    TabInactiveBackground,
    TabActiveBackground,
    SearchMatchBackground,
    SearchActiveMatchBackground,
    PanelBackground,
    PanelFocusedBorder,
    PanelIndentGuide,
    PanelIndentGuideHover,
    PanelIndentGuideActive,
    PanelOverlayBackground,
    PanelOverlayHover,
    PaneFocusedBorder,
    PaneGroupBorder,
    ScrollbarThumbBackground,
    ScrollbarThumbHoverBackground,
    ScrollbarThumbActiveBackground,
    ScrollbarThumbBorder,
    ScrollbarTrackBackground,
    ScrollbarTrackBorder,
    EditorForeground,
    EditorBackground,
    EditorGutterBackground,
    EditorSubheaderBackground,
    EditorActiveLineBackground,
    EditorHighlightedLineBackground,
    EditorLineNumber,
    EditorActiveLineNumber,
    EditorInvisible,
    EditorWrapGuide,
    EditorActiveWrapGuide,
    EditorIndentGuide,
    EditorIndentGuideActive,
    EditorDocumentHighlightReadBackground,
    EditorDocumentHighlightWriteBackground,
    EditorDocumentHighlightBracketBackground,
    LinkTextHover,
    VersionControlAdded,
    VersionControlDeleted,
    VersionControlModified,
    VersionControlRenamed,
    VersionControlConflict,
    VersionControlIgnored,
}

impl ThemeColors {
    pub fn color(&self, field: ThemeColorField) -> Hsla {
        match field {
            ThemeColorField::Border => self.border,
            ThemeColorField::BorderVariant => self.border_variant,
            ThemeColorField::BorderFocused => self.border_focused,
            ThemeColorField::BorderSelected => self.border_selected,
            ThemeColorField::BorderTransparent => self.border_transparent,
            ThemeColorField::BorderDisabled => self.border_disabled,
            ThemeColorField::ElevatedSurfaceBackground => self.elevated_surface_background,
            ThemeColorField::SurfaceBackground => self.surface_background,
            ThemeColorField::Background => self.background,
            ThemeColorField::ElementBackground => self.element_background,
            ThemeColorField::ElementHover => self.element_hover,
            ThemeColorField::ElementActive => self.element_active,
            ThemeColorField::ElementSelected => self.element_selected,
            ThemeColorField::ElementDisabled => self.element_disabled,
            ThemeColorField::DropTargetBackground => self.drop_target_background,
            ThemeColorField::DropTargetBorder => self.drop_target_border,
            ThemeColorField::GhostElementBackground => self.ghost_element_background,
            ThemeColorField::GhostElementHover => self.ghost_element_hover,
            ThemeColorField::GhostElementActive => self.ghost_element_active,
            ThemeColorField::GhostElementSelected => self.ghost_element_selected,
            ThemeColorField::GhostElementDisabled => self.ghost_element_disabled,
            ThemeColorField::Text => self.text,
            ThemeColorField::TextMuted => self.text_muted,
            ThemeColorField::TextPlaceholder => self.text_placeholder,
            ThemeColorField::TextDisabled => self.text_disabled,
            ThemeColorField::TextAccent => self.text_accent,
            ThemeColorField::Icon => self.icon,
            ThemeColorField::IconMuted => self.icon_muted,
            ThemeColorField::IconDisabled => self.icon_disabled,
            ThemeColorField::IconPlaceholder => self.icon_placeholder,
            ThemeColorField::IconAccent => self.icon_accent,
            ThemeColorField::StatusBarBackground => self.status_bar_background,
            ThemeColorField::TitleBarBackground => self.title_bar_background,
            ThemeColorField::TitleBarInactiveBackground => self.title_bar_inactive_background,
            ThemeColorField::ToolbarBackground => self.toolbar_background,
            ThemeColorField::TabBarBackground => self.tab_bar_background,
            ThemeColorField::TabInactiveBackground => self.tab_inactive_background,
            ThemeColorField::TabActiveBackground => self.tab_active_background,
            ThemeColorField::SearchMatchBackground => self.search_match_background,
            ThemeColorField::SearchActiveMatchBackground => self.search_active_match_background,
            ThemeColorField::PanelBackground => self.panel_background,
            ThemeColorField::PanelFocusedBorder => self.panel_focused_border,
            ThemeColorField::PanelIndentGuide => self.panel_indent_guide,
            ThemeColorField::PanelIndentGuideHover => self.panel_indent_guide_hover,
            ThemeColorField::PanelIndentGuideActive => self.panel_indent_guide_active,
            ThemeColorField::PanelOverlayBackground => self.panel_overlay_background,
            ThemeColorField::PanelOverlayHover => self.panel_overlay_hover,
            ThemeColorField::PaneFocusedBorder => self.pane_focused_border,
            ThemeColorField::PaneGroupBorder => self.pane_group_border,
            ThemeColorField::ScrollbarThumbBackground => self.scrollbar_thumb_background,
            ThemeColorField::ScrollbarThumbHoverBackground => self.scrollbar_thumb_hover_background,
            ThemeColorField::ScrollbarThumbActiveBackground => {
                self.scrollbar_thumb_active_background
            }
            ThemeColorField::ScrollbarThumbBorder => self.scrollbar_thumb_border,
            ThemeColorField::ScrollbarTrackBackground => self.scrollbar_track_background,
            ThemeColorField::ScrollbarTrackBorder => self.scrollbar_track_border,
            ThemeColorField::EditorForeground => self.editor_foreground,
            ThemeColorField::EditorBackground => self.editor_background,
            ThemeColorField::EditorGutterBackground => self.editor_gutter_background,
            ThemeColorField::EditorSubheaderBackground => self.editor_subheader_background,
            ThemeColorField::EditorActiveLineBackground => self.editor_active_line_background,
            ThemeColorField::EditorHighlightedLineBackground => {
                self.editor_highlighted_line_background
            }
            ThemeColorField::EditorLineNumber => self.editor_line_number,
            ThemeColorField::EditorActiveLineNumber => self.editor_active_line_number,
            ThemeColorField::EditorInvisible => self.editor_invisible,
            ThemeColorField::EditorWrapGuide => self.editor_wrap_guide,
            ThemeColorField::EditorActiveWrapGuide => self.editor_active_wrap_guide,
            ThemeColorField::EditorIndentGuide => self.editor_indent_guide,
            ThemeColorField::EditorIndentGuideActive => self.editor_indent_guide_active,
            ThemeColorField::EditorDocumentHighlightReadBackground => {
                self.editor_document_highlight_read_background
            }
            ThemeColorField::EditorDocumentHighlightWriteBackground => {
                self.editor_document_highlight_write_background
            }
            ThemeColorField::EditorDocumentHighlightBracketBackground => {
                self.editor_document_highlight_bracket_background
            }
            ThemeColorField::LinkTextHover => self.link_text_hover,
            ThemeColorField::VersionControlAdded => self.version_control_added,
            ThemeColorField::VersionControlDeleted => self.version_control_deleted,
            ThemeColorField::VersionControlModified => self.version_control_modified,
            ThemeColorField::VersionControlRenamed => self.version_control_renamed,
            ThemeColorField::VersionControlConflict => self.version_control_conflict,
            ThemeColorField::VersionControlIgnored => self.version_control_ignored,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (ThemeColorField, Hsla)> + '_ {
        ThemeColorField::iter().map(move |field| (field, self.color(field)))
    }

    pub fn to_vec(&self) -> Vec<(ThemeColorField, Hsla)> {
        self.iter().collect()
    }
}

pub fn all_theme_colors(cx: &mut App) -> Vec<(Hsla, SharedString)> {
    let theme = cx.theme();
    ThemeColorField::iter()
        .map(|field| {
            let color = theme.colors().color(field);
            let name = field.as_ref().to_string();
            (color, SharedString::from(name))
        })
        .collect()
}

#[derive(Refineable, Clone, Debug, PartialEq)]
pub struct ThemeStyles {
    /// The background appearance of the window.
    pub window_background_appearance: WindowBackgroundAppearance,
    pub system: SystemColors,
    /// An array of colors used for theme elements that iterate through a series of colors.
    ///
    /// Example: Player colors, rainbow brackets and indent guides, etc.
    pub accents: AccentColors,

    #[refineable]
    pub colors: ThemeColors,

    #[refineable]
    pub status: StatusColors,

    pub player: PlayerColors,

    pub syntax: Arc<SyntaxTheme>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn override_a_single_theme_color() {
        let mut colors = ThemeColors::light();

        let magenta: Hsla = gpui::rgb(0xff00ff).into();

        assert_ne!(colors.text, magenta);

        let overrides = ThemeColorsRefinement {
            text: Some(magenta),
            ..Default::default()
        };

        colors.refine(&overrides);

        assert_eq!(colors.text, magenta);
    }

    #[test]
    fn override_multiple_theme_colors() {
        let mut colors = ThemeColors::light();

        let magenta: Hsla = gpui::rgb(0xff00ff).into();
        let green: Hsla = gpui::rgb(0x00ff00).into();

        assert_ne!(colors.text, magenta);
        assert_ne!(colors.background, green);

        let overrides = ThemeColorsRefinement {
            text: Some(magenta),
            background: Some(green),
            ..Default::default()
        };

        colors.refine(&overrides);

        assert_eq!(colors.text, magenta);
        assert_eq!(colors.background, green);
    }

    #[test]
    fn deserialize_theme_colors_refinement_from_json() {
        let colors: ThemeColorsRefinement = serde_json::from_value(json!({
            "background": "#ff00ff",
            "text": "#ff0000"
        }))
        .unwrap();

        assert_eq!(colors.background, Some(gpui::rgb(0xff00ff).into()));
        assert_eq!(colors.text, Some(gpui::rgb(0xff0000).into()));
    }
}
