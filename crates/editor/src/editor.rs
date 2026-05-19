#![allow(rustdoc::private_intra_doc_links)]
//! This is the place where everything editor-related is stored (data-wise) and displayed (ui-wise).
//! The main point of interest in this crate is [`Editor`] type, which is used in every other Zen part as a user input element.
//! It comes in different flavors: single line, multiline and a fixed height one.
//!
//! Editor contains of multiple large submodules:
//! * [`element`] — the place where all rendering happens
//! * [`display_map`] - chunks up text in the editor into the logical blocks, establishes coordinates and mapping between each of them.
//!   Contains all metadata related to text transformations (folds, fake inlay text insertions, soft wraps, tab markup, etc.).
//!
//! All other submodules and structs are mostly concerned with holding editor data about the way it displays current buffer region(s).
//!
pub mod actions;
mod authorship;
pub mod blink_manager;
mod bracket_colorization;
pub mod display_map;
mod document_symbols;
mod editor_settings;
mod element;
mod highlight_matching_bracket;
mod hover_links;
pub mod hover_popover;
mod indent_guides;
mod inlays;
pub mod items;
mod mouse_context_menu;
pub mod movement;
pub mod scroll;
mod selections_collection;

pub(crate) use actions::*;
pub use display_map::{
    ChunkRenderer, ChunkRendererContext, DisplayPoint, FoldPlaceholder, HighlightKey,
    NavigationOverlayKey,
};
pub use editor_settings::{
    CurrentLineHighlight, EditorSettings, EditorSettingsScrollbarProxy, HideMouseMode,
    ScrollBeyondLastLine, ScrollbarAxes, ui_scrollbar_settings_from_raw,
};
pub use element::{
    CursorLayout, EditorElement, HighlightedRange, HighlightedRangeLine, PointForPosition,
};
pub use inlays::Inlay;
pub use items::MAX_TAB_TITLE_LEN;
pub use multi_buffer::{
    Anchor, AnchorRangeExt, BufferOffset, ExcerptRange, MBTextSummary, MultiBuffer,
    MultiBufferOffset, MultiBufferOffsetUtf16, MultiBufferSnapshot, PathKey, RowInfo, ToOffset,
    ToPoint,
};
pub use text::Bias;

use anyhow::Result;
use blink_manager::BlinkManager;
use collections::{BTreeMap, HashMap, HashSet, VecDeque};
use display_map::*;
use element::{LineWithInvisibles, PositionMap};
use gpui::{
    Action, AnyElement, App, AppContext, AsyncWindowContext, Background, Bounds, ClipboardItem,
    Context, DispatchPhase, Entity, EntityId, EntityInputHandler, EventEmitter, FocusHandle,
    FocusOutEvent, Focusable, FontId, FontStyle, HighlightStyle, Hsla, KeyContext, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, PaintQuad, ParentElement, Pixels, PressureStage,
    Render, SharedString, Size, Styled, Subscription, Task, TextRun, TextStyle,
    TextStyleRefinement, UTF16Selection, UnderlineStyle, WeakEntity, WeakFocusHandle, Window, div,
    point, prelude::*, px, relative, size,
};
use hover_links::{HoverLink, HoveredLinkState};
use hover_popover::{HoverState, hide_hover};
use indent_guides::ActiveIndentGuidesState;
use language::{
    AuthorshipSource, AutoindentMode, BlockCommentConfig, BracketMatch, BracketPair, Buffer,
    BufferRow, BufferSnapshot, Capability, CursorShape, IndentKind, IndentSize, Language,
    LanguageAwareStyling, LanguageName, LanguageScope, LocalFile, OffsetRangeExt, OutlineItem,
    Point, Selection, SelectionGoal, TextObject, TransactionId, TreeSitterOptions,
    language_settings::{self, LanguageSettings},
};
use mouse_context_menu::MouseContextMenu;
use movement::TextLayoutDetails;
use multi_buffer::{ExcerptBoundaryInfo, ExpandExcerptDirection, MultiBufferPoint, MultiBufferRow};
use project::{Project, ProjectPath, ProjectTransaction};
use regex::Regex;
use scroll::{Autoscroll, OngoingScroll, ScrollAnchor, ScrollManager, SharedScrollAnchor};
use selections_collection::{MutableSelectionsCollection, SelectionsCollection};
use serde::{Deserialize, Serialize};
use settings::{
    RelativeLineNumbers, Settings, SettingsLocation, SettingsStore, update_settings_file,
};
use smallvec::{SmallVec, smallvec};
use std::{
    any::{Any, TypeId},
    borrow::Cow,
    cell::RefCell,
    cmp::{self, Ordering},
    collections::hash_map,
    mem,
    num::NonZeroU32,
    ops::{Deref, Not, Range, RangeInclusive},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use text::{BufferId, OffsetUtf16, Rope, ToPoint as _};
use theme::{
    AccentColors, ActiveTheme, GlobalTheme, PlayerColor, StatusColors, SyntaxTheme, Theme,
};
use theme_settings::{ThemeSettings, observe_buffer_font_size_adjustment};
use ui::{ContextMenu, Disclosure, prelude::*, scrollbars::ScrollbarAutoHide};
use ui_input::ErasedEditor;
use util::{RangeExt, ResultExt, maybe, post_inc};
use workspace::{
    Item as WorkspaceItem, ItemNavHistory, SplitDirection, TabBarSettings, Workspace,
    item::ItemBufferKind,
    notifications::{DetachAndPromptErr, NotifyTaskExt},
};
pub use zen_actions::editor::RevealInFileManager;
use zen_actions::editor::{MoveDown, MoveUp};

use crate::{
    editor_settings::MultiCursorModifier,
    scroll::{ScrollOffset, ScrollPixelOffset},
    selections_collection::resolve_selections_wrapping_blocks,
};

pub const FILE_HEADER_HEIGHT: u32 = 2;
pub const BUFFER_HEADER_PADDING: Rems = rems(0.25);
pub const MULTI_BUFFER_EXCERPT_HEADER_HEIGHT: u32 = 1;
const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(500);
const MAX_LINE_LEN: usize = 1024;
const MAX_SELECTION_HISTORY_LEN: usize = 1024;
pub const SELECTION_HIGHLIGHT_DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(100);

pub(crate) const SCROLL_CENTER_TOP_BOTTOM_DEBOUNCE_TIMEOUT: Duration = Duration::from_secs(1);
pub(crate) const EDIT_PREDICTION_KEY_CONTEXT: &str = "edit_prediction";

enum ReportEditorEvent {
    Saved,
    EditorOpened,
    Closed,
}

pub enum ConflictsOuter {}
pub enum ConflictsOurs {}
pub enum ConflictsTheirs {}
pub enum ConflictsOursMarker {}
pub enum ConflictsTheirsMarker {}

pub struct HunkAddedColor;
pub struct HunkRemovedColor;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Navigated {
    Yes,
    No,
}

impl Navigated {
    pub fn from_bool(yes: bool) -> Navigated {
        if yes { Navigated::Yes } else { Navigated::No }
    }
}

pub enum HideMouseCursorOrigin {
    TypingAction,
    MovementAction,
}

pub fn init(cx: &mut App) {
    workspace::register_project_item::<Editor>(cx);

    cx.observe_new(
        |workspace: &mut Workspace, _: Option<&mut Window>, _cx: &mut Context<Workspace>| {
            workspace.register_action(Editor::new_file);
            workspace.register_action(Editor::new_file_split);
            workspace.register_action(Editor::new_file_vertical);
            workspace.register_action(Editor::new_file_horizontal);
            workspace.register_action(Editor::toggle_focus);
        },
    )
    .detach();

    cx.on_action(move |_: &workspace::NewFile, cx| {
        let app_state = workspace::AppState::global(cx);
        workspace::open_new(
            Default::default(),
            app_state,
            cx,
            |workspace, window, cx| Editor::new_file(workspace, &Default::default(), window, cx),
        )
        .detach_and_log_err(cx);
    })
    .on_action(move |_: &workspace::NewWindow, cx| {
        let app_state = workspace::AppState::global(cx);
        workspace::open_new(
            Default::default(),
            app_state,
            cx,
            |workspace, window, cx| {
                cx.activate(true);
                Editor::new_file(workspace, &Default::default(), window, cx)
            },
        )
        .detach_and_log_err(cx);
    });
    _ = ui_input::ERASED_EDITOR_FACTORY.set(|window, cx| {
        Arc::new(ErasedEditorImpl(
            cx.new(|cx| Editor::single_line(window, cx)),
        )) as Arc<dyn ErasedEditor>
    });
    _ = multi_buffer::EXCERPT_CONTEXT_LINES.set(multibuffer_context_lines);
}

#[derive(Clone, Debug, PartialEq)]
pub enum SelectPhase {
    Begin {
        position: DisplayPoint,
        add: bool,
        click_count: usize,
    },
    BeginColumnar {
        position: DisplayPoint,
        reset: bool,
        mode: ColumnarMode,
        goal_column: u32,
    },
    Extend {
        position: DisplayPoint,
        click_count: usize,
    },
    Update {
        position: DisplayPoint,
        goal_column: u32,
        scroll_delta: gpui::Point<f32>,
    },
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ColumnarMode {
    FromMouse,
    FromSelection,
}

#[derive(Clone, Debug)]
pub enum SelectMode {
    Character,
    Word(Range<Anchor>),
    Line(Range<Anchor>),
    All,
}

#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub enum SizingBehavior {
    /// The editor will layout itself using `size_full` and will include the vertical
    /// scroll margin as requested by user settings.
    #[default]
    Default,
    /// The editor will layout itself using `size_full`, but will not have any
    /// vertical overscroll.
    ExcludeOverscrollMargin,
    /// The editor will request a vertical size according to its content and will be
    /// layouted without a vertical scroll margin.
    SizeByContent,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EditorMode {
    SingleLine,
    AutoHeight {
        min_lines: usize,
        max_lines: Option<usize>,
    },
    Full {
        /// When set to `true`, the editor will scale its UI elements with the buffer font size.
        scale_ui_elements_with_buffer_font_size: bool,
        /// When set to `true`, the editor will render a background for the active line.
        show_active_line_background: bool,
        /// Determines the sizing behavior for this editor
        sizing_behavior: SizingBehavior,
    },
}

impl EditorMode {
    pub fn full() -> Self {
        Self::Full {
            scale_ui_elements_with_buffer_font_size: true,
            show_active_line_background: true,
            sizing_behavior: SizingBehavior::Default,
        }
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Full { .. })
    }

    #[inline]
    pub fn is_single_line(&self) -> bool {
        matches!(self, Self::SingleLine { .. })
    }
}

#[derive(Copy, Clone, Debug)]
pub enum SoftWrap {
    /// Prefer not to wrap at all.
    ///
    /// Note: this is currently internal, as actually limited by [`crate::MAX_LINE_LEN`] until it wraps.
    /// The mode is used inside git diff hunks, where it's seems currently more useful to not wrap as much as possible.
    GitDiff,
    /// Prefer a single line generally, unless an overly long line is encountered.
    None,
    /// Soft wrap lines that exceed the editor width.
    EditorWidth,
    /// Soft wrap line at the preferred line length or the editor width (whichever is smaller).
    Bounded(u32),
}

#[derive(Clone)]
pub struct EditorStyle {
    pub background: Hsla,
    pub border: Hsla,
    pub local_player: PlayerColor,
    pub text: TextStyle,
    pub scrollbar_width: Pixels,
    pub syntax: Arc<SyntaxTheme>,
    pub status: StatusColors,
    pub inlay_style: HighlightStyle,
    pub edit_prediction_styles: EditPredictionStyles,
    pub unnecessary_code_fade: f32,
    pub show_underlines: bool,
}

impl Default for EditorStyle {
    fn default() -> Self {
        Self {
            background: Hsla::default(),
            border: Hsla::default(),
            local_player: PlayerColor::default(),
            text: TextStyle::default(),
            scrollbar_width: Pixels::default(),
            syntax: Default::default(),
            // HACK: Status colors don't have a real default.
            // We should look into removing the status colors from the editor
            // style and retrieve them directly from the theme.
            status: StatusColors::dark(),
            inlay_style: HighlightStyle::default(),
            edit_prediction_styles: EditPredictionStyles {
                insertion: HighlightStyle::default(),
                whitespace: HighlightStyle::default(),
            },
            unnecessary_code_fade: Default::default(),
            show_underlines: true,
        }
    }
}

pub fn make_inlay_style(cx: &App) -> HighlightStyle {
    let mut style = cx
        .theme()
        .syntax()
        .style_for_name("hint")
        .unwrap_or_default();

    if style.color.is_none() {
        style.color = Some(cx.theme().status().hint);
    }

    style.background_color = None;

    style
}

pub fn make_suggestion_styles(cx: &App) -> EditPredictionStyles {
    EditPredictionStyles {
        insertion: HighlightStyle {
            color: Some(cx.theme().status().predictive),
            ..HighlightStyle::default()
        },
        whitespace: HighlightStyle {
            background_color: Some(cx.theme().status().created_background),
            ..HighlightStyle::default()
        },
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Direction {
    Prev,
    Next,
}

#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Debug, Default)]
struct EditorActionId(usize);

impl EditorActionId {
    pub fn post_inc(&mut self) -> Self {
        let answer = self.0;

        *self = Self(answer + 1);

        Self(answer)
    }
}

// type GetFieldEditorTheme = dyn Fn(&theme::Theme) -> theme::FieldEditor;
// type OverrideTextStyle = dyn Fn(&EditorStyle) -> Option<HighlightStyle>;

type BackgroundHighlight = (
    Arc<dyn Fn(&usize, &Theme) -> Hsla + Send + Sync>,
    Arc<[Range<Anchor>]>,
);
type GutterHighlight = (fn(&App) -> Hsla, Vec<Range<Anchor>>);

#[derive(Default)]
struct ScrollbarMarkerState {
    scrollbar_size: Size<Pixels>,
    dirty: bool,
    markers: Arc<[PaintQuad]>,
    pending_refresh: Option<Task<Result<()>>>,
}

impl ScrollbarMarkerState {
    fn should_refresh(&self, scrollbar_size: Size<Pixels>) -> bool {
        self.pending_refresh.is_none() && (self.scrollbar_size != scrollbar_size || self.dirty)
    }
}

/// Addons allow storing per-editor state in other crates.
pub trait Addon: 'static {
    fn extend_key_context(&self, _: &mut KeyContext, _: &App) {}

    fn render_buffer_header_controls(
        &self,
        _: &ExcerptBoundaryInfo,
        _: &language::BufferSnapshot,
        _: &Window,
        _: &App,
    ) -> Option<AnyElement> {
        None
    }

    fn extend_buffer_header_context_menu(
        &self,
        menu: ui::ContextMenu,
        _: &language::BufferSnapshot,
        _: &mut Window,
        _: &mut App,
    ) -> ui::ContextMenu {
        menu
    }

    fn to_any(&self) -> &dyn std::any::Any;

    fn to_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
}

struct ChangeLocation {
    current: Option<Vec<Anchor>>,
    original: Vec<Anchor>,
}
impl ChangeLocation {
    fn locations(&self) -> &[Anchor] {
        self.current.as_ref().unwrap_or(&self.original)
    }
}

/// A set of caret positions, registered when the editor was edited.
pub struct ChangeList {
    changes: Vec<ChangeLocation>,
    /// Currently "selected" change.
    position: Option<usize>,
}

impl ChangeList {
    pub fn new() -> Self {
        Self {
            changes: Vec::new(),
            position: None,
        }
    }

    /// Moves to the next change in the list (based on the direction given) and returns the caret positions for the next change.
    /// If reaches the end of the list in the direction, returns the corresponding change until called for a different direction.
    pub fn next_change(&mut self, count: usize, direction: Direction) -> Option<&[Anchor]> {
        if self.changes.is_empty() {
            return None;
        }

        let prev = self.position.unwrap_or(self.changes.len());
        let next = if direction == Direction::Prev {
            prev.saturating_sub(count)
        } else {
            (prev + count).min(self.changes.len() - 1)
        };
        self.position = Some(next);
        self.changes.get(next).map(|change| change.locations())
    }

    /// Adds a new change to the list, resetting the change list position.
    pub fn push_to_change_list(&mut self, group: bool, new_positions: Vec<Anchor>) {
        self.position.take();
        if let Some(last) = self.changes.last_mut()
            && group
        {
            last.current = Some(new_positions)
        } else {
            self.changes.push(ChangeLocation {
                original: new_positions,
                current: None,
            });
        }
    }

    pub fn last(&self) -> Option<&[Anchor]> {
        self.changes.last().map(|change| change.locations())
    }

    pub fn last_before_grouping(&self) -> Option<&[Anchor]> {
        self.changes.last().map(|change| change.original.as_slice())
    }

    pub fn invert_last_group(&mut self) {
        if let Some(last) = self.changes.last_mut()
            && let Some(current) = last.current.as_mut()
        {
            mem::swap(&mut last.original, current);
        }
    }
}

enum SelectionDragState {
    /// State when no drag related activity is detected.
    None,
    /// State when the mouse is down on a selection that is about to be dragged.
    ReadyToDrag {
        selection: Selection<Anchor>,
        click_position: gpui::Point<Pixels>,
        mouse_down_time: Instant,
    },
    /// State when the mouse is dragging the selection in the editor.
    Dragging {
        selection: Selection<Anchor>,
        drop_cursor: Selection<Anchor>,
        hide_drop_cursor: bool,
    },
}

enum ColumnarSelectionState {
    FromMouse {
        selection_tail: Anchor,
        display_point: Option<DisplayPoint>,
    },
    FromSelection {
        selection_tail: Anchor,
    },
}

/// Zen's primary implementation of text input, allowing users to edit a [`MultiBuffer`].
///
/// See the [module level documentation](self) for more information.
pub struct Editor {
    focus_handle: FocusHandle,
    last_focused_descendant: Option<WeakFocusHandle>,
    /// The text buffer being edited
    buffer: Entity<MultiBuffer>,
    /// Map of how text in the buffer should be displayed.
    /// Handles soft wraps, folds, fake inlay text insertions, etc.
    pub display_map: Entity<DisplayMap>,
    placeholder_display_map: Option<Entity<DisplayMap>>,
    pub selections: SelectionsCollection,
    pub scroll_manager: ScrollManager,
    /// When inline assist editors are linked, they all render cursors because
    /// typing enters text into each of them, even the ones that aren't focused.
    pub(crate) show_cursor_when_unfocused: bool,
    columnar_selection_state: Option<ColumnarSelectionState>,
    selection_history: SelectionHistory,
    defer_selection_effects: bool,
    deferred_selection_effects_state: Option<DeferredSelectionEffectsState>,
    autoclose_regions: Vec<AutocloseRegion>,
    ime_transaction: Option<TransactionId>,
    soft_wrap_mode_override: Option<language_settings::SoftWrap>,
    hard_wrap: Option<usize>,
    project: Option<Entity<Project>>,
    blink_manager: Entity<BlinkManager>,
    mode: EditorMode,
    show_gutter: bool,
    show_scrollbars: ScrollbarAxes,
    offset_content: bool,
    disable_expand_excerpt_buttons: bool,
    delegate_expand_excerpts: bool,
    delegate_open_excerpts: bool,
    enable_mouse_wheel_zoom: bool,
    show_line_numbers: Option<bool>,
    use_relative_line_numbers: Option<bool>,
    show_wrap_guides: Option<bool>,
    show_indent_guides: Option<bool>,
    buffers_with_disabled_indent_guides: HashSet<BufferId>,
    highlight_order: usize,
    highlighted_rows: HashMap<TypeId, Vec<RowHighlight>>,
    background_highlights: HashMap<HighlightKey, BackgroundHighlight>,
    navigation_overlays: HashMap<NavigationOverlayKey, Arc<[NavigationTargetOverlay]>>,
    gutter_highlights: HashMap<TypeId, GutterHighlight>,
    scrollbar_marker_state: ScrollbarMarkerState,
    active_indent_guides_state: ActiveIndentGuidesState,
    nav_history: Option<ItemNavHistory>,
    mouse_context_menu: Option<MouseContextMenu>,
    last_selection_from_search: bool,
    cursor_shape: CursorShape,
    /// Whether the cursor is offset one character to the left when something is selected.
    cursor_offset_on_selection: bool,
    current_line_highlight: Option<CurrentLineHighlight>,
    /// Whether to collapse search match ranges to just their start position.
    /// When true, navigating to a match positions the cursor at the match
    /// without selecting the matched text.
    collapse_matches: bool,
    autoindent_mode: Option<AutoindentMode>,
    workspace: Option<WeakEntity<Workspace>>,
    input_enabled: bool,
    expects_character_input: bool,
    use_modal_editing: bool,
    read_only: bool,
    pub hover_state: HoverState,
    pending_mouse_down: Option<Rc<RefCell<Option<MouseDownEvent>>>>,
    prev_pressure_stage: Option<PressureStage>,
    gutter_hovered: bool,
    hovered_link_state: Option<HoveredLinkState>,
    in_leading_whitespace: bool,
    _subscriptions: Vec<Subscription>,
    pixel_position_of_newest_cursor: Option<gpui::Point<Pixels>>,
    gutter_dimensions: GutterDimensions,
    style: Option<EditorStyle>,
    text_style_refinement: Option<TextStyleRefinement>,
    next_editor_action_id: EditorActionId,
    editor_actions: Rc<
        RefCell<BTreeMap<EditorActionId, Box<dyn Fn(&Editor, &mut Window, &mut Context<Self>)>>>,
    >,
    use_autoclose: bool,
    use_auto_surround: bool,
    use_selection_highlight: bool,
    custom_context_menu: Option<
        Box<
            dyn 'static
                + Fn(
                    &mut Self,
                    DisplayPoint,
                    &mut Window,
                    &mut Context<Self>,
                ) -> Option<Entity<ui::ContextMenu>>,
        >,
    >,
    last_bounds: Option<Bounds<Pixels>>,
    last_position_map: Option<Rc<PositionMap>>,
    expect_bounds_change: Option<Bounds<Pixels>>,
    focused_block: Option<FocusedBlock>,
    next_scroll_position: NextScrollCursorCenterTopBottom,
    addons: HashMap<TypeId, Box<dyn Addon>>,
    selection_mark_mode: bool,
    _scroll_cursor_center_top_bottom_task: Task<()>,
    serialize_authorship: Task<()>,
    show_authorship: bool,
    mouse_cursor_hidden: bool,
    hide_mouse_mode: HideMouseMode,
    pub change_list: ChangeList,

    selection_drag_state: SelectionDragState,
    post_scroll_update: Task<()>,
    folding_newlines: Task<()>,
    pub lookup_key: Option<Box<dyn Any + Send + Sync>>,
    on_local_selections_changed:
        Option<Box<dyn Fn(Point, &mut Window, &mut Context<Self>) + 'static>>,
    suppress_selection_callback: bool,
    applicable_language_settings: HashMap<Option<LanguageName>, LanguageSettings>,
    accent_data: Option<AccentData>,
    bracket_fetched_tree_sitter_chunks: HashMap<Range<text::Anchor>, HashSet<Range<BufferRow>>>,
    pub(crate) refresh_matching_bracket_highlights_task: Task<()>,
    sticky_headers_task: Task<()>,
    sticky_headers: Option<Vec<OutlineItem<Anchor>>>,
    pub(crate) colorize_brackets_task: Task<()>,
}

#[derive(Debug, PartialEq)]
struct AccentData {
    colors: AccentColors,
    overrides: Vec<SharedString>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
enum NextScrollCursorCenterTopBottom {
    #[default]
    Center,
    Top,
    Bottom,
}

impl NextScrollCursorCenterTopBottom {
    fn next(&self) -> Self {
        match self {
            Self::Center => Self::Top,
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Center,
        }
    }
}

#[derive(Clone)]
pub struct EditorSnapshot {
    pub mode: EditorMode,
    show_gutter: bool,
    offset_content: bool,
    show_line_numbers: Option<bool>,
    pub display_snapshot: DisplaySnapshot,
    pub placeholder_display_snapshot: Option<DisplaySnapshot>,
    is_focused: bool,
    scroll_anchor: SharedScrollAnchor,
    ongoing_scroll: OngoingScroll,
    current_line_highlight: CurrentLineHighlight,
    gutter_hovered: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavigationTargetOverlay {
    pub target_range: Range<Anchor>,
    pub label: NavigationOverlayLabel,
    pub covered_text_range: Option<Range<Anchor>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavigationOverlayLabel {
    pub text: SharedString,
    pub text_color: Hsla,
    pub x_offset: Pixels,
    pub scale_factor: f32,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct GutterDimensions {
    pub left_padding: Pixels,
    pub right_padding: Pixels,
    pub width: Pixels,
    pub margin: Pixels,
}

impl GutterDimensions {
    fn default_with_margin(font_id: FontId, font_size: Pixels, cx: &App) -> Self {
        Self {
            margin: Self::default_gutter_margin(font_id, font_size, cx),
            ..Default::default()
        }
    }

    fn default_gutter_margin(font_id: FontId, font_size: Pixels, cx: &App) -> Pixels {
        -cx.text_system().descent(font_id, font_size)
    }
    /// The full width of the space taken up by the gutter.
    pub fn full_width(&self) -> Pixels {
        self.margin + self.width
    }

    /// The width of the space reserved for the fold indicators,
    /// use alongside 'justify_end' and `gutter_width` to
    /// right align content with the line numbers
    pub fn fold_area_width(&self) -> Pixels {
        self.margin + self.right_padding
    }
}

struct CharacterDimensions {
    em_width: Pixels,
    em_advance: Pixels,
    line_height: Pixels,
}

#[derive(Clone, Debug)]
struct SelectionHistoryEntry {
    selections: Arc<[Selection<Anchor>]>,
}

#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
enum SelectionHistoryMode {
    #[default]
    Normal,
    Skipping,
}

#[derive(Debug)]
/// SelectionEffects controls the side-effects of updating the selection.
///
/// The default behaviour does "what you mostly want":
/// - it pushes to the nav history if the cursor moved by >10 lines
/// - it scrolls to fit
///
/// You might want to modify these behaviours. For example when doing a "jump"
/// like go to definition, we always want to add to nav history.
///
/// Similarly, you might want to disable scrolling if you don't want the viewport to
/// move.
#[derive(Clone)]
pub struct SelectionEffects {
    nav_history: Option<bool>,
    scroll: Option<Autoscroll>,
    from_search: bool,
}

impl Default for SelectionEffects {
    fn default() -> Self {
        Self {
            nav_history: None,
            scroll: Some(Autoscroll::fit()),
            from_search: false,
        }
    }
}
impl SelectionEffects {
    pub fn scroll(scroll: Autoscroll) -> Self {
        Self {
            scroll: Some(scroll),
            ..Default::default()
        }
    }

    pub fn no_scroll() -> Self {
        Self {
            scroll: None,
            ..Default::default()
        }
    }

    pub fn nav_history(self, nav_history: bool) -> Self {
        Self {
            nav_history: Some(nav_history),
            ..self
        }
    }

    pub fn from_search(self, from_search: bool) -> Self {
        Self {
            from_search,
            ..self
        }
    }
}

struct DeferredSelectionEffectsState {
    changed: bool,
    effects: SelectionEffects,
    old_cursor_position: Anchor,
    history_entry: SelectionHistoryEntry,
}

#[derive(Default)]
struct SelectionHistory {
    #[allow(clippy::type_complexity)]
    selections_by_transaction:
        HashMap<TransactionId, (Arc<[Selection<Anchor>]>, Option<Arc<[Selection<Anchor>]>>)>,
    mode: SelectionHistoryMode,
    undo_stack: VecDeque<SelectionHistoryEntry>,
}

impl SelectionHistory {
    #[track_caller]
    fn insert_transaction(
        &mut self,
        transaction_id: TransactionId,
        selections: Arc<[Selection<Anchor>]>,
    ) {
        if selections.is_empty() {
            log::error!(
                "SelectionHistory::insert_transaction called with empty selections. Caller: {}",
                std::panic::Location::caller()
            );
            return;
        }
        self.selections_by_transaction
            .insert(transaction_id, (selections, None));
    }

    #[allow(clippy::type_complexity)]
    fn transaction(
        &self,
        transaction_id: TransactionId,
    ) -> Option<&(Arc<[Selection<Anchor>]>, Option<Arc<[Selection<Anchor>]>>)> {
        self.selections_by_transaction.get(&transaction_id)
    }

    #[allow(clippy::type_complexity)]
    fn transaction_mut(
        &mut self,
        transaction_id: TransactionId,
    ) -> Option<&mut (Arc<[Selection<Anchor>]>, Option<Arc<[Selection<Anchor>]>>)> {
        self.selections_by_transaction.get_mut(&transaction_id)
    }

    fn push(&mut self, entry: SelectionHistoryEntry) {
        if !entry.selections.is_empty() {
            match self.mode {
                SelectionHistoryMode::Normal => {
                    self.push_undo(entry);
                }
                SelectionHistoryMode::Skipping => {}
            }
        }
    }

    fn push_undo(&mut self, entry: SelectionHistoryEntry) {
        if self
            .undo_stack
            .back()
            .is_none_or(|e| e.selections != entry.selections)
        {
            self.undo_stack.push_back(entry);
            if self.undo_stack.len() > MAX_SELECTION_HISTORY_LEN {
                self.undo_stack.pop_front();
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct RowHighlightOptions {
    pub autoscroll: bool,
    pub include_gutter: bool,
}

impl Default for RowHighlightOptions {
    fn default() -> Self {
        Self {
            autoscroll: Default::default(),
            include_gutter: true,
        }
    }
}

struct RowHighlight {
    index: usize,
    range: Range<Anchor>,
    color: Hsla,
    options: RowHighlightOptions,
    type_id: TypeId,
}

#[derive(Debug)]
struct AutocloseRegion {
    selection_id: usize,
    range: Range<Anchor>,
    pair: BracketPair,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClipboardSelection {
    /// The number of bytes in this selection.
    pub len: usize,
    /// Whether this was a full-line selection.
    pub is_entire_line: bool,
    /// The indentation of the first line when this content was originally copied.
    pub first_line_indent: u32,
    #[serde(default)]
    pub file_path: Option<PathBuf>,
    #[serde(default)]
    pub line_range: Option<RangeInclusive<u32>>,
}

impl ClipboardSelection {
    pub fn for_buffer(
        len: usize,
        is_entire_line: bool,
        range: Range<Point>,
        buffer: &MultiBufferSnapshot,
        project: Option<&Entity<Project>>,
        cx: &App,
    ) -> Self {
        let first_line_indent = buffer
            .indent_size_for_line(MultiBufferRow(range.start.row))
            .len;

        let file_path = util::maybe!({
            let project = project?.read(cx);
            let file = buffer.file_at(range.start)?;
            let project_path = ProjectPath {
                worktree_id: file.worktree_id(cx),
                path: file.path().clone(),
            };
            project.absolute_path(&project_path, cx)
        });

        let line_range = if file_path.is_some() {
            buffer
                .range_to_buffer_range(range)
                .map(|(_, buffer_range)| buffer_range.start.row..=buffer_range.end.row)
        } else {
            None
        };

        Self {
            len,
            is_entire_line,
            first_line_indent,
            file_path,
            line_range,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct NavigationData {
    cursor_anchor: Anchor,
    cursor_position: Point,
    scroll_anchor: ScrollAnchor,
    scroll_top_row: u32,
}

pub(crate) struct FocusedBlock {
    id: BlockId,
    focus_handle: WeakFocusHandle,
}

#[derive(Clone, Debug)]
pub enum JumpData {
    MultiBufferRow {
        row: MultiBufferRow,
        line_offset_from_top: u32,
    },
    MultiBufferPoint {
        anchor: language::Anchor,
        position: Point,
        line_offset_from_top: u32,
    },
}

pub enum MultibufferSelectionMode {
    First,
    All,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RewrapOptions {
    pub override_language_settings: bool,
    pub preserve_existing_whitespace: bool,
    pub line_length: Option<usize>,
}

impl Editor {
    pub fn single_line(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let buffer = cx.new(|cx| Buffer::local("", cx));
        let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Self::new(EditorMode::SingleLine, buffer, None, window, cx)
    }

    pub fn multi_line(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let buffer = cx.new(|cx| Buffer::local("", cx));
        let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Self::new(EditorMode::full(), buffer, None, window, cx)
    }

    pub fn auto_height(
        min_lines: usize,
        max_lines: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let buffer = cx.new(|cx| Buffer::local("", cx));
        let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Self::new(
            EditorMode::AutoHeight {
                min_lines,
                max_lines: Some(max_lines),
            },
            buffer,
            None,
            window,
            cx,
        )
    }

    /// Creates a new auto-height editor with a minimum number of lines but no maximum.
    /// The editor grows as tall as needed to fit its content.
    pub fn auto_height_unbounded(
        min_lines: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let buffer = cx.new(|cx| Buffer::local("", cx));
        let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Self::new(
            EditorMode::AutoHeight {
                min_lines,
                max_lines: None,
            },
            buffer,
            None,
            window,
            cx,
        )
    }

    pub fn for_buffer(
        buffer: Entity<Buffer>,
        project: Option<Entity<Project>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Self::new(EditorMode::full(), buffer, project, window, cx)
    }

    pub fn for_multibuffer(
        buffer: Entity<MultiBuffer>,
        project: Option<Entity<Project>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new(EditorMode::full(), buffer, project, window, cx)
    }

    pub fn clone(&self, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut clone = Self::new(
            self.mode.clone(),
            self.buffer.clone(),
            self.project.clone(),
            window,
            cx,
        );
        let my_snapshot = self.display_map.update(cx, |display_map, cx| {
            let snapshot = display_map.snapshot(cx);
            clone.display_map.update(cx, |display_map, cx| {
                display_map.set_state(&snapshot, cx);
            });
            snapshot
        });
        let clone_snapshot = clone.display_map.update(cx, |map, cx| map.snapshot(cx));
        clone.selections.clone_state(&self.selections);
        clone
            .scroll_manager
            .clone_state(&self.scroll_manager, &my_snapshot, &clone_snapshot, cx);
        clone.read_only = self.read_only;
        clone.buffers_with_disabled_indent_guides =
            self.buffers_with_disabled_indent_guides.clone();
        clone.enable_mouse_wheel_zoom = self.enable_mouse_wheel_zoom;
        clone
    }

    pub fn new(
        mode: EditorMode,
        buffer: Entity<MultiBuffer>,
        project: Option<Entity<Project>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Editor::new_internal(mode, buffer, project, None, window, cx)
    }

    pub fn refresh_sticky_headers(
        &mut self,
        display_snapshot: &DisplaySnapshot,
        cx: &mut Context<Editor>,
    ) {
        if !self.mode.is_full() {
            return;
        }
        let multi_buffer = display_snapshot.buffer_snapshot().clone();
        let scroll_anchor = self
            .scroll_manager
            .native_anchor(display_snapshot, cx)
            .anchor;
        let Some(buffer_snapshot) = multi_buffer.as_singleton() else {
            return;
        };

        let buffer = buffer_snapshot.clone();
        let Some((buffer_visible_start, _)) = multi_buffer.anchor_to_buffer_anchor(scroll_anchor)
        else {
            return;
        };
        let buffer_visible_start = buffer_visible_start.to_point(&buffer);
        let max_row = buffer.max_point().row;
        let start_row = buffer_visible_start.row.min(max_row);
        let end_row = (buffer_visible_start.row + 10).min(max_row);

        let syntax = self.style(cx).syntax.clone();
        let background_task = cx.background_spawn(async move {
            buffer
                .outline_items_containing(
                    Point::new(start_row, 0)..Point::new(end_row, 0),
                    true,
                    Some(syntax.as_ref()),
                )
                .into_iter()
                .filter_map(|outline_item| {
                    Some(OutlineItem {
                        depth: outline_item.depth,
                        range: multi_buffer
                            .buffer_anchor_range_to_anchor_range(outline_item.range)?,
                        source_range_for_text: multi_buffer.buffer_anchor_range_to_anchor_range(
                            outline_item.source_range_for_text,
                        )?,
                        text: outline_item.text,
                        highlight_ranges: outline_item.highlight_ranges,
                        name_ranges: outline_item.name_ranges,
                        body_range: outline_item.body_range.and_then(|range| {
                            multi_buffer.buffer_anchor_range_to_anchor_range(range)
                        }),
                        annotation_range: outline_item.annotation_range.and_then(|range| {
                            multi_buffer.buffer_anchor_range_to_anchor_range(range)
                        }),
                    })
                })
                .collect()
        });
        self.sticky_headers_task = cx.spawn(async move |this, cx| {
            let sticky_headers = background_task.await;
            this.update(cx, |this, cx| {
                this.sticky_headers = Some(sticky_headers);
                cx.notify();
            })
            .ok();
        });
    }

    fn new_internal(
        mode: EditorMode,
        multi_buffer: Entity<MultiBuffer>,
        project: Option<Entity<Project>>,
        display_map: Option<Entity<DisplayMap>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        debug_assert!(display_map.is_none());

        let full_mode = mode.is_full();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let editor = cx.entity().downgrade();
        let fold_placeholder = FoldPlaceholder {
            constrain_width: false,
            render: Arc::new(move |fold_id, fold_range, cx| {
                let editor = editor.clone();
                FoldPlaceholder::fold_element(fold_id, cx)
                    .cursor_pointer()
                    .child("⋯")
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _window, cx| {
                        editor
                            .update(cx, |editor, cx| {
                                editor.unfold_ranges(
                                    &[fold_range.start..fold_range.end],
                                    true,
                                    false,
                                    cx,
                                );
                                cx.stop_propagation();
                            })
                            .ok();
                    })
                    .into_any()
            }),
            merge_adjacent: true,
            ..FoldPlaceholder::default()
        };
        let display_map = display_map.unwrap_or_else(|| {
            cx.new(|cx| {
                DisplayMap::new(
                    multi_buffer.clone(),
                    style.font(),
                    font_size,
                    None,
                    FILE_HEADER_HEIGHT,
                    MULTI_BUFFER_EXCERPT_HEADER_HEIGHT,
                    fold_placeholder,
                    cx,
                )
            })
        });

        let selections = SelectionsCollection::new();

        let blink_manager = cx.new(|cx| {
            BlinkManager::new(
                CURSOR_BLINK_INTERVAL,
                |cx| EditorSettings::get_global(cx).cursor_blink,
                cx,
            )
        });

        let soft_wrap_mode_override =
            matches!(mode, EditorMode::SingleLine).then(|| language_settings::SoftWrap::None);

        let mut project_subscriptions = Vec::new();
        if full_mode && let Some(project) = project.as_ref() {
            project_subscriptions.push(cx.subscribe_in(
                project,
                window,
                |editor, _, event, window, cx| match event {
                    project::Event::EntryRenamed(transaction, project_path, abs_path) => {
                        let Some(workspace) = editor.workspace() else {
                            return;
                        };
                        let Some(active_editor) = workspace.read(cx).active_item_as::<Self>(cx)
                        else {
                            return;
                        };

                        if active_editor.entity_id() == cx.entity_id() {
                            let entity_id = cx.entity_id();
                            workspace.update(cx, |this, cx| {
                                this.panes_mut()
                                    .iter_mut()
                                    .filter(|pane| pane.entity_id() != entity_id)
                                    .for_each(|p| {
                                        p.update(cx, |pane, _| {
                                            pane.nav_history_mut().rename_item(
                                                entity_id,
                                                project_path.clone(),
                                                abs_path.clone().into(),
                                            );
                                        })
                                    });
                            });

                            Self::open_transaction_for_hidden_buffers(
                                workspace,
                                transaction.clone(),
                                "Rename".to_string(),
                                window,
                                cx,
                            );
                        }
                    }

                    _ => {}
                },
            ));
        }

        let focus_handle = cx.focus_handle();
        cx.on_focus(&focus_handle, window, Self::handle_focus)
            .detach();
        cx.on_focus_in(&focus_handle, window, Self::handle_focus_in)
            .detach();
        cx.on_focus_out(&focus_handle, window, Self::handle_focus_out)
            .detach();
        cx.on_blur(&focus_handle, window, Self::handle_blur)
            .detach();
        cx.observe_pending_input(window, Self::observe_pending_input)
            .detach();

        let show_indent_guides = if matches!(mode, EditorMode::SingleLine) {
            Some(false)
        } else {
            None
        };

        let mut editor = Self {
            focus_handle,
            show_cursor_when_unfocused: false,
            last_focused_descendant: None,
            buffer: multi_buffer.clone(),
            display_map: display_map.clone(),
            placeholder_display_map: None,
            selections,
            scroll_manager: ScrollManager::new(cx),
            columnar_selection_state: None,
            selection_history: SelectionHistory::default(),
            defer_selection_effects: false,
            deferred_selection_effects_state: None,
            autoclose_regions: Vec::new(),
            ime_transaction: None,
            soft_wrap_mode_override,
            hard_wrap: None,
            project,
            blink_manager: blink_manager.clone(),
            show_scrollbars: ScrollbarAxes {
                horizontal: full_mode,
                vertical: full_mode,
            },
            offset_content: !matches!(mode, EditorMode::SingleLine),
            show_gutter: full_mode,
            show_line_numbers: (!full_mode).then_some(false),
            use_relative_line_numbers: None,
            disable_expand_excerpt_buttons: !full_mode,
            delegate_expand_excerpts: false,
            delegate_open_excerpts: false,
            enable_mouse_wheel_zoom: full_mode,
            show_wrap_guides: None,
            show_indent_guides,
            buffers_with_disabled_indent_guides: HashSet::default(),
            highlight_order: 0,
            highlighted_rows: HashMap::default(),
            background_highlights: HashMap::default(),
            navigation_overlays: HashMap::default(),
            gutter_highlights: HashMap::default(),
            scrollbar_marker_state: ScrollbarMarkerState::default(),
            active_indent_guides_state: ActiveIndentGuidesState::default(),
            nav_history: None,
            mouse_context_menu: None,
            last_selection_from_search: false,
            cursor_shape: EditorSettings::get_global(cx)
                .cursor_shape
                .unwrap_or_default(),
            cursor_offset_on_selection: false,
            current_line_highlight: None,
            autoindent_mode: Some(AutoindentMode::EachLine),
            collapse_matches: false,
            workspace: None,
            input_enabled: true,
            expects_character_input: true,
            use_modal_editing: full_mode,
            read_only: false,
            use_autoclose: true,
            use_auto_surround: true,
            use_selection_highlight: true,
            hover_state: HoverState::default(),
            pending_mouse_down: None,
            prev_pressure_stage: None,
            hovered_link_state: None,
            gutter_hovered: false,
            pixel_position_of_newest_cursor: None,
            last_bounds: None,
            last_position_map: None,
            expect_bounds_change: None,
            gutter_dimensions: GutterDimensions::default(),
            style: None,
            next_editor_action_id: EditorActionId::default(),
            editor_actions: Rc::default(),
            in_leading_whitespace: false,
            custom_context_menu: None,
            _subscriptions: vec![
                cx.observe(&multi_buffer, Self::on_buffer_changed),
                cx.subscribe_in(&multi_buffer, window, Self::on_buffer_event),
                cx.observe_in(&display_map, window, Self::on_display_map_changed),
                cx.observe(&blink_manager, |_, _, cx| cx.notify()),
                cx.observe_global_in::<SettingsStore>(window, Self::settings_changed),
                cx.observe_global_in::<GlobalTheme>(window, Self::theme_changed),
                observe_buffer_font_size_adjustment(cx, |_, cx| cx.notify()),
                cx.observe_window_activation(window, |editor, window, cx| {
                    let active = window.is_window_active();
                    editor.blink_manager.update(cx, |blink_manager, cx| {
                        if active {
                            blink_manager.enable(cx);
                        } else {
                            blink_manager.disable(cx);
                        }
                    });
                    if active {
                        editor.show_mouse_cursor(cx);
                    }
                }),
            ],
            post_scroll_update: Task::ready(()),
            focused_block: None,
            next_scroll_position: NextScrollCursorCenterTopBottom::default(),
            addons: HashMap::default(),
            _scroll_cursor_center_top_bottom_task: Task::ready(()),
            selection_mark_mode: false,
            serialize_authorship: Task::ready(()),
            show_authorship: false,
            text_style_refinement: None,
            mouse_cursor_hidden: false,
            hide_mouse_mode: EditorSettings::get_global(cx)
                .hide_mouse
                .unwrap_or_default(),
            change_list: ChangeList::new(),
            mode,
            selection_drag_state: SelectionDragState::None,
            folding_newlines: Task::ready(()),
            lookup_key: None,
            on_local_selections_changed: None,
            suppress_selection_callback: false,
            applicable_language_settings: HashMap::default(),
            accent_data: None,
            bracket_fetched_tree_sitter_chunks: HashMap::default(),
            refresh_matching_bracket_highlights_task: Task::ready(()),
            sticky_headers_task: Task::ready(()),
            sticky_headers: None,
            colorize_brackets_task: Task::ready(()),
        };

        editor.applicable_language_settings = editor.fetch_applicable_language_settings(cx);
        editor.accent_data = editor.fetch_accent_data(cx);
        editor.restore_authorship(cx);

        editor._subscriptions.extend(project_subscriptions);

        editor._subscriptions.push(cx.subscribe_in(
            &cx.entity(),
            window,
            |editor, _, e: &EditorEvent, window, cx| match e {
                EditorEvent::ScrollPositionChanged { .. } => {
                    editor.update_data_on_scroll(true, window, cx);
                    editor.refresh_sticky_headers(&editor.snapshot(window, cx), cx);
                }
                EditorEvent::Edited { .. } => {
                    let display_map = editor.display_snapshot(cx);
                    let selections = editor.selections.all_adjusted_display(&display_map);
                    let pop_state = editor
                        .change_list
                        .last()
                        .map(|previous| {
                            previous.len() == selections.len()
                                && previous.iter().enumerate().all(|(ix, p)| {
                                    p.to_display_point(&display_map).row()
                                        == selections[ix].head().row()
                                })
                        })
                        .unwrap_or(false);
                    let new_positions = selections
                        .into_iter()
                        .map(|s| display_map.display_point_to_anchor(s.head(), Bias::Left))
                        .collect();
                    editor
                        .change_list
                        .push_to_change_list(pop_state, new_positions);
                }
                _ => (),
            },
        ));

        // skip adding the initial selection to selection history
        editor.selection_history.mode = SelectionHistoryMode::Skipping;
        editor.end_selection(window, cx);
        editor.selection_history.mode = SelectionHistoryMode::Normal;

        editor.scroll_manager.show_scrollbars(window, cx);

        if full_mode {
            let should_auto_hide_scrollbars = cx.should_auto_hide_scrollbars();
            cx.set_global(ScrollbarAutoHide(should_auto_hide_scrollbars));

            editor.report_editor_event(ReportEditorEvent::EditorOpened, None, cx);
        }

        editor
    }

    pub fn display_snapshot(&self, cx: &mut App) -> DisplaySnapshot {
        self.display_map.update(cx, |map, cx| map.snapshot(cx))
    }

    pub fn deploy_mouse_context_menu(
        &mut self,
        position: gpui::Point<Pixels>,
        context_menu: Entity<ContextMenu>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mouse_context_menu = Some(MouseContextMenu::new(
            self,
            crate::mouse_context_menu::MenuPosition::PinnedToScreen(position),
            context_menu,
            window,
            cx,
        ));
    }

    pub fn mouse_menu_is_focused(&self, window: &Window, cx: &App) -> bool {
        self.mouse_context_menu
            .as_ref()
            .is_some_and(|menu| menu.context_menu.focus_handle(cx).is_focused(window))
    }

    pub fn is_range_selected(&mut self, range: &Range<Anchor>, cx: &mut Context<Self>) -> bool {
        if self
            .selections
            .pending_anchor()
            .is_some_and(|pending_selection| {
                let snapshot = self.buffer().read(cx).snapshot(cx);
                pending_selection.range().includes(range, &snapshot)
            })
        {
            return true;
        }

        self.selections
            .disjoint_in_range::<MultiBufferOffset>(range.clone(), &self.display_snapshot(cx))
            .into_iter()
            .any(|selection| {
                // This is needed to cover a corner case, if we just check for an existing
                // selection in the fold range, having a cursor at the start of the fold
                // marks it as selected. Non-empty selections don't cause this.
                let length = selection.end - selection.start;
                length > 0
            })
    }

    pub fn key_context(&self, window: &mut Window, cx: &mut App) -> KeyContext {
        self.key_context_internal(self.has_active_edit_prediction(), window, cx)
    }

    fn key_context_internal(
        &self,
        has_active_edit_prediction: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> KeyContext {
        let mut key_context = KeyContext::new_with_defaults();
        key_context.add("Editor");
        let mode = match self.mode {
            EditorMode::SingleLine => "single_line",
            EditorMode::AutoHeight { .. } => "auto_height",
            EditorMode::Full { .. } => "full",
        };

        key_context.set("mode", mode);
        // Let addons extend key context only for the focused editor.
        if !self.focus_handle(cx).contains_focused(window, cx)
            || (self.is_focused(window) || self.mouse_menu_is_focused(window, cx))
        {
            for addon in self.addons.values() {
                addon.extend_key_context(&mut key_context, cx)
            }
        }

        if let Some(singleton_buffer) = self.buffer.read(cx).as_singleton() {
            if let Some(extension) = singleton_buffer.read(cx).file().and_then(|file| {
                Some(
                    file.full_path(cx)
                        .extension()?
                        .to_string_lossy()
                        .to_lowercase(),
                )
            }) {
                key_context.set("extension", extension);
            }
        } else {
            key_context.add("multibuffer");
        }

        if has_active_edit_prediction {
            key_context.add(EDIT_PREDICTION_KEY_CONTEXT);
            key_context.add("copilot_suggestion");
        }

        if self.in_leading_whitespace {
            key_context.add("in_leading_whitespace");
        }
        if self.edit_prediction_requires_modifier() {
            key_context.set("edit_prediction_mode", "subtle")
        } else {
            key_context.set("edit_prediction_mode", "eager");
        }

        if self.selection_mark_mode {
            key_context.add("selection_mode");
        }

        let disjoint = self.selections.disjoint_anchors();
        if matches!(
            &self.mode,
            EditorMode::SingleLine | EditorMode::AutoHeight { .. }
        ) && let [selection] = disjoint
            && selection.start == selection.end
        {
            let snapshot = self.snapshot(window, cx);
            let snapshot = snapshot.buffer_snapshot();
            let caret_offset = selection.end.to_offset(snapshot);

            if caret_offset == MultiBufferOffset(0) {
                key_context.add("start_of_input");
            }

            if caret_offset == snapshot.len() {
                key_context.add("end_of_input");
            }
        }

        key_context
    }

    pub fn last_bounds(&self) -> Option<&Bounds<Pixels>> {
        self.last_bounds.as_ref()
    }

    fn show_mouse_cursor(&mut self, cx: &mut Context<Self>) {
        if self.mouse_cursor_hidden {
            self.mouse_cursor_hidden = false;
            cx.notify();
        }
    }

    pub fn hide_mouse_cursor(&mut self, origin: HideMouseCursorOrigin, cx: &mut Context<Self>) {
        let hide_mouse_cursor = match origin {
            HideMouseCursorOrigin::TypingAction => {
                matches!(
                    self.hide_mouse_mode,
                    HideMouseMode::OnTyping | HideMouseMode::OnTypingAndMovement
                )
            }
            HideMouseCursorOrigin::MovementAction => {
                matches!(self.hide_mouse_mode, HideMouseMode::OnTypingAndMovement)
            }
        };
        if self.mouse_cursor_hidden != hide_mouse_cursor {
            self.mouse_cursor_hidden = hide_mouse_cursor;
            cx.notify();
        }
    }

    pub fn new_file(
        workspace: &mut Workspace,
        _: &workspace::NewFile,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::new_in_workspace(workspace, window, cx).detach_and_prompt_err(
            "Failed to create buffer",
            window,
            cx,
            |_, _, _| None,
        );
    }

    pub fn new_in_workspace(
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Task<Result<Entity<Editor>>> {
        let project = workspace.project().clone();
        let create = project.update(cx, |project, cx| project.create_buffer(None, cx));

        cx.spawn_in(window, async move |workspace, cx| {
            let buffer = create.await?;
            workspace.update_in(cx, |workspace, window, cx| {
                let editor =
                    cx.new(|cx| Editor::for_buffer(buffer, Some(project.clone()), window, cx));
                workspace.add_item_to_active_pane(Box::new(editor.clone()), None, true, window, cx);
                editor
            })
        })
    }

    fn new_file_vertical(
        workspace: &mut Workspace,
        _: &workspace::NewFileSplitVertical,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::new_file_in_direction(workspace, SplitDirection::vertical(cx), window, cx)
    }

    fn new_file_horizontal(
        workspace: &mut Workspace,
        _: &workspace::NewFileSplitHorizontal,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::new_file_in_direction(workspace, SplitDirection::horizontal(cx), window, cx)
    }

    fn new_file_split(
        workspace: &mut Workspace,
        action: &workspace::NewFileSplit,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::new_file_in_direction(workspace, action.0, window, cx)
    }

    fn new_file_in_direction(
        workspace: &mut Workspace,
        direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let project = workspace.project().clone();
        let create = project.update(cx, |project, cx| project.create_buffer(None, cx));

        cx.spawn_in(window, async move |workspace, cx| {
            let buffer = create.await?;
            workspace.update_in(cx, move |workspace, window, cx| {
                workspace.split_item(
                    direction,
                    Box::new(
                        cx.new(|cx| Editor::for_buffer(buffer, Some(project.clone()), window, cx)),
                    ),
                    window,
                    cx,
                )
            })?;
            anyhow::Ok(())
        })
        .detach_and_prompt_err("Failed to create buffer", window, cx, |_, _, _| None);
    }

    pub fn buffer(&self) -> &Entity<MultiBuffer> {
        &self.buffer
    }

    pub fn project(&self) -> Option<&Entity<Project>> {
        self.project.as_ref()
    }

    pub fn workspace(&self) -> Option<Entity<Workspace>> {
        self.workspace.as_ref()?.upgrade()
    }

    /// Detaches a task and shows an error notification in the workspace if available,
    /// otherwise just logs the error.
    pub fn detach_and_notify_err<R, E>(
        &self,
        task: Task<Result<R, E>>,
        window: &mut Window,
        cx: &mut App,
    ) where
        E: std::fmt::Debug + std::fmt::Display + 'static,
        R: 'static,
    {
        if let Some(workspace) = self.workspace() {
            task.detach_and_notify_err(workspace.downgrade(), window, cx);
        } else {
            task.detach_and_log_err(cx);
        }
    }

    pub fn title<'a>(&self, cx: &'a App) -> Cow<'a, str> {
        self.buffer().read(cx).title(cx)
    }

    pub fn snapshot(&self, window: &Window, cx: &mut App) -> EditorSnapshot {
        let display_snapshot = self.display_map.update(cx, |map, cx| map.snapshot(cx));

        EditorSnapshot {
            mode: self.mode.clone(),
            show_gutter: self.show_gutter,
            offset_content: self.offset_content,
            show_line_numbers: self.show_line_numbers,
            scroll_anchor: self.scroll_manager.shared_scroll_anchor(cx),
            display_snapshot,
            placeholder_display_snapshot: self
                .placeholder_display_map
                .as_ref()
                .map(|display_map| display_map.update(cx, |map, cx| map.snapshot(cx))),
            ongoing_scroll: self.scroll_manager.ongoing_scroll(),
            is_focused: self.focus_handle.is_focused(window),
            current_line_highlight: self
                .current_line_highlight
                .unwrap_or_else(|| EditorSettings::get_global(cx).current_line_highlight),
            gutter_hovered: self.gutter_hovered,
        }
    }

    pub fn language_at<T: ToOffset>(&self, point: T, cx: &App) -> Option<Arc<Language>> {
        self.buffer.read(cx).language_at(point, cx)
    }

    pub fn file_at<T: ToOffset>(&self, point: T, cx: &App) -> Option<Arc<dyn language::File>> {
        self.buffer.read(cx).read(cx).file_at(point).cloned()
    }

    pub fn active_buffer(&self, cx: &App) -> Option<Entity<Buffer>> {
        let multibuffer = self.buffer.read(cx);
        let snapshot = multibuffer.snapshot(cx);
        let (anchor, _) =
            snapshot.anchor_to_buffer_anchor(self.selections.newest_anchor().head())?;
        multibuffer.buffer(anchor.buffer_id)
    }

    pub fn mode(&self) -> &EditorMode {
        &self.mode
    }

    pub fn set_mode(&mut self, mode: EditorMode) {
        self.mode = mode;
    }

    pub fn set_custom_context_menu(
        &mut self,
        f: impl 'static
        + Fn(
            &mut Self,
            DisplayPoint,
            &mut Window,
            &mut Context<Self>,
        ) -> Option<Entity<ui::ContextMenu>>,
    ) {
        self.custom_context_menu = Some(Box::new(f))
    }

    pub fn placeholder_text(&self, cx: &mut App) -> Option<String> {
        self.placeholder_display_map
            .as_ref()
            .map(|display_map| display_map.update(cx, |map, cx| map.snapshot(cx)).text())
    }

    pub fn set_placeholder_text(
        &mut self,
        placeholder_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let multibuffer = cx
            .new(|cx| MultiBuffer::singleton(cx.new(|cx| Buffer::local(placeholder_text, cx)), cx));

        let style = window.text_style();

        self.placeholder_display_map = Some(cx.new(|cx| {
            DisplayMap::new(
                multibuffer,
                style.font(),
                style.font_size.to_pixels(window.rem_size()),
                None,
                FILE_HEADER_HEIGHT,
                MULTI_BUFFER_EXCERPT_HEADER_HEIGHT,
                Default::default(),
                cx,
            )
        }));
        cx.notify();
    }

    pub fn set_cursor_shape(&mut self, cursor_shape: CursorShape, cx: &mut Context<Self>) {
        self.cursor_shape = cursor_shape;

        // Disrupt blink for immediate user feedback that the cursor shape has changed
        self.blink_manager.update(cx, BlinkManager::show_cursor);

        cx.notify();
    }

    pub fn show_cursor(&mut self, cx: &mut Context<Self>) {
        self.blink_manager.update(cx, BlinkManager::show_cursor);
    }

    pub fn cursor_shape(&self) -> CursorShape {
        self.cursor_shape
    }

    pub fn set_cursor_offset_on_selection(&mut self, set_cursor_offset_on_selection: bool) {
        self.cursor_offset_on_selection = set_cursor_offset_on_selection;
    }

    pub fn set_current_line_highlight(
        &mut self,
        current_line_highlight: Option<CurrentLineHighlight>,
    ) {
        self.current_line_highlight = current_line_highlight;
    }

    pub fn set_collapse_matches(&mut self, collapse_matches: bool) {
        self.collapse_matches = collapse_matches;
    }

    pub fn range_for_match<T: std::marker::Copy>(&self, range: &Range<T>) -> Range<T> {
        if self.collapse_matches {
            return range.start..range.start;
        }
        range.clone()
    }

    pub fn clip_at_line_ends(&mut self, cx: &mut Context<Self>) -> bool {
        self.display_map.read(cx).clip_at_line_ends
    }

    pub fn set_clip_at_line_ends(&mut self, clip: bool, cx: &mut Context<Self>) {
        if self.display_map.read(cx).clip_at_line_ends != clip {
            self.display_map
                .update(cx, |map, _| map.clip_at_line_ends = clip);
        }
    }

    pub fn set_input_enabled(&mut self, input_enabled: bool) {
        self.input_enabled = input_enabled;
    }

    pub fn set_expects_character_input(&mut self, expects_character_input: bool) {
        self.expects_character_input = expects_character_input;
    }

    pub fn set_autoindent(&mut self, autoindent: bool) {
        if autoindent {
            self.autoindent_mode = Some(AutoindentMode::EachLine);
        } else {
            self.autoindent_mode = None;
        }
    }

    pub fn capability(&self, cx: &App) -> Capability {
        if self.read_only {
            Capability::ReadOnly
        } else {
            self.buffer.read(cx).capability()
        }
    }

    pub fn read_only(&self, cx: &App) -> bool {
        self.read_only || self.buffer.read(cx).read_only()
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    pub fn set_use_autoclose(&mut self, autoclose: bool) {
        self.use_autoclose = autoclose;
    }

    pub fn set_use_selection_highlight(&mut self, highlight: bool) {
        self.use_selection_highlight = highlight;
    }

    pub fn set_use_auto_surround(&mut self, auto_surround: bool) {
        self.use_auto_surround = auto_surround;
    }

    pub fn set_auto_replace_emoji_shortcode(&mut self, _auto_replace: bool) {}

    pub fn set_use_modal_editing(&mut self, to: bool) {
        self.use_modal_editing = to;
    }

    pub fn use_modal_editing(&self) -> bool {
        self.use_modal_editing
    }

    fn selections_did_change(
        &mut self,
        local: bool,
        old_cursor_position: &Anchor,
        effects: SelectionEffects,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.last_selection_from_search = effects.from_search;
        window.invalidate_character_coordinates();

        // Copy selections to primary selection buffer
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        if local {
            let selections = self
                .selections
                .all::<MultiBufferOffset>(&self.display_snapshot(cx));
            let buffer_handle = self.buffer.read(cx).read(cx);

            let mut text = String::new();
            for (index, selection) in selections.iter().enumerate() {
                let text_for_selection = buffer_handle
                    .text_for_range(selection.start..selection.end)
                    .collect::<String>();

                text.push_str(&text_for_selection);
                if index != selections.len() - 1 {
                    text.push('\n');
                }
            }

            if !text.is_empty() {
                cx.write_to_primary(ClipboardItem::new_string(text));
            }
        }

        let selection_anchors = self.selections.disjoint_anchors_arc();

        if self.focus_handle.is_focused(window) {
            self.buffer.update(cx, |buffer, cx| {
                buffer.set_active_selections(
                    &selection_anchors,
                    self.selections.line_mode(),
                    self.cursor_shape,
                    cx,
                )
            });
        }
        let display_map = self
            .display_map
            .update(cx, |display_map, cx| display_map.snapshot(cx));
        let buffer = display_map.buffer_snapshot();
        self.invalidate_autoclose_regions(&selection_anchors, buffer);

        let newest_selection = self.selections.newest_anchor();
        let new_cursor_position = newest_selection.head();

        if effects.nav_history.is_none() || effects.nav_history == Some(true) {
            self.push_to_nav_history(
                *old_cursor_position,
                Some(new_cursor_position.to_point(buffer)),
                false,
                effects.nav_history == Some(true),
                cx,
            );
        }

        if local {
            hide_hover(self, cx);

            self.refresh_selected_text_highlights(&display_map, false, window, cx);
            self.refresh_matching_bracket_highlights(&display_map, cx);
            self.update_visible_edit_prediction(window, cx);
        }

        self.blink_manager.update(cx, BlinkManager::pause_blinking);

        if local && !self.suppress_selection_callback {
            if let Some(callback) = self.on_local_selections_changed.as_ref() {
                let cursor_position = self.selections.newest::<Point>(&display_map).head();
                callback(cursor_position, window, cx);
            }
        }

        cx.emit(EditorEvent::SelectionsChanged { local });

        cx.notify();
    }

    pub fn sync_selections(
        &mut self,
        other: Entity<Editor>,
        cx: &mut Context<Self>,
    ) -> gpui::Subscription {
        let other_selections = other.read(cx).selections.disjoint_anchors().to_vec();
        if !other_selections.is_empty() {
            self.selections
                .change_with(&self.display_snapshot(cx), |selections| {
                    selections.select_anchors(other_selections);
                });
        }

        let other_subscription = cx.subscribe(&other, |this, other, other_evt, cx| {
            if let EditorEvent::SelectionsChanged { local: true } = other_evt {
                let other_selections = other.read(cx).selections.disjoint_anchors().to_vec();
                if other_selections.is_empty() {
                    return;
                }
                let snapshot = this.display_snapshot(cx);
                this.selections.change_with(&snapshot, |selections| {
                    selections.select_anchors(other_selections);
                });
            }
        });

        let this_subscription = cx.subscribe_self::<EditorEvent>(move |this, this_evt, cx| {
            if let EditorEvent::SelectionsChanged { local: true } = this_evt {
                let these_selections = this.selections.disjoint_anchors().to_vec();
                if these_selections.is_empty() {
                    return;
                }
                other.update(cx, |other_editor, cx| {
                    let snapshot = other_editor.display_snapshot(cx);
                    other_editor
                        .selections
                        .change_with(&snapshot, |selections| {
                            selections.select_anchors(these_selections);
                        })
                });
            }
        });

        Subscription::join(other_subscription, this_subscription)
    }

    fn unfold_buffers_with_selections(&mut self, cx: &mut Context<Self>) {
        if self.buffer().read(cx).is_singleton() {
            return;
        }
        let snapshot = self.buffer.read(cx).snapshot(cx);
        let buffer_ids: HashSet<BufferId> = self
            .selections
            .disjoint_anchor_ranges()
            .flat_map(|range| snapshot.buffer_ids_for_range(range))
            .collect();
        for buffer_id in buffer_ids {
            self.unfold_buffer(buffer_id, cx);
        }
    }

    /// Changes selections using the provided mutation function. Changes to `self.selections` occur
    /// immediately, but when run within `transact` or `with_selection_effects_deferred` other
    /// effects of selection change occur at the end of the transaction.
    pub fn change_selections<R>(
        &mut self,
        effects: SelectionEffects,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut MutableSelectionsCollection<'_, '_>) -> R,
    ) -> R {
        let snapshot = self.display_snapshot(cx);
        if let Some(state) = &mut self.deferred_selection_effects_state {
            state.effects.scroll = effects.scroll.or(state.effects.scroll);
            state.effects.nav_history = effects.nav_history.or(state.effects.nav_history);
            let (changed, result) = self.selections.change_with(&snapshot, change);
            state.changed |= changed;
            return result;
        }
        let mut state = DeferredSelectionEffectsState {
            changed: false,
            effects,
            old_cursor_position: self.selections.newest_anchor().head(),
            history_entry: SelectionHistoryEntry {
                selections: self.selections.disjoint_anchors_arc(),
            },
        };
        let (changed, result) = self.selections.change_with(&snapshot, change);
        state.changed = state.changed || changed;
        if self.defer_selection_effects {
            self.deferred_selection_effects_state = Some(state);
        } else {
            self.apply_selection_effects(state, window, cx);
        }
        result
    }

    /// Defers the effects of selection change, so that the effects of multiple calls to
    /// `change_selections` are applied at the end. This way these intermediate states aren't added
    /// to selection history and the state of popovers based on selection position aren't
    /// erroneously updated.
    pub fn with_selection_effects_deferred<R>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) -> R,
    ) -> R {
        let already_deferred = self.defer_selection_effects;
        self.defer_selection_effects = true;
        let result = update(self, window, cx);
        if !already_deferred {
            self.defer_selection_effects = false;
            if let Some(state) = self.deferred_selection_effects_state.take() {
                self.apply_selection_effects(state, window, cx);
            }
        }
        result
    }

    fn apply_selection_effects(
        &mut self,
        state: DeferredSelectionEffectsState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if state.changed {
            self.selection_history.push(state.history_entry);

            if let Some(autoscroll) = state.effects.scroll {
                self.request_autoscroll(autoscroll, cx);
            }

            let old_cursor_position = &state.old_cursor_position;

            self.selections_did_change(true, old_cursor_position, state.effects, window, cx);
        }
    }

    pub fn edit<I, S, T>(&mut self, edits: I, cx: &mut Context<Self>)
    where
        I: IntoIterator<Item = (Range<S>, T)>,
        S: ToOffset,
        T: Into<Arc<str>>,
    {
        if self.read_only(cx) {
            return;
        }

        self.buffer
            .update(cx, |buffer, cx| buffer.edit(edits, None, cx));
    }

    pub fn edit_with_authorship<I, S, T>(
        &mut self,
        edits: I,
        authorship_source: AuthorshipSource,
        cx: &mut Context<Self>,
    ) where
        I: IntoIterator<Item = (Range<S>, T)>,
        S: ToOffset,
        T: Into<Arc<str>>,
    {
        if self.read_only(cx) {
            return;
        }

        self.buffer.update(cx, |buffer, cx| {
            buffer.edit_with_authorship(edits, None, authorship_source, cx)
        });
    }

    pub fn edit_with_autoindent<I, S, T>(&mut self, edits: I, cx: &mut Context<Self>)
    where
        I: IntoIterator<Item = (Range<S>, T)>,
        S: ToOffset,
        T: Into<Arc<str>>,
    {
        if self.read_only(cx) {
            return;
        }

        self.buffer.update(cx, |buffer, cx| {
            buffer.edit(edits, self.autoindent_mode.clone(), cx)
        });
    }

    pub fn edit_with_autoindent_and_authorship<I, S, T>(
        &mut self,
        edits: I,
        authorship_source: AuthorshipSource,
        cx: &mut Context<Self>,
    ) where
        I: IntoIterator<Item = (Range<S>, T)>,
        S: ToOffset,
        T: Into<Arc<str>>,
    {
        if self.read_only(cx) {
            return;
        }

        self.buffer.update(cx, |buffer, cx| {
            buffer.edit_with_authorship(edits, self.autoindent_mode.clone(), authorship_source, cx)
        });
    }

    fn restore_authorship(&mut self, cx: &mut Context<Self>) {
        let Some((buffer, path)) = self.singleton_authorship_buffer_and_path(cx) else {
            return;
        };

        let Some(persisted) = authorship::load(&path).log_err().flatten() else {
            return;
        };

        buffer.update(cx, |buffer, _| {
            buffer.restore_human_authorship_ranges(
                persisted.previous_text(),
                persisted.human_ranges(),
            );
        });
        self.persist_authorship(cx);
    }

    fn persist_authorship(&mut self, cx: &mut Context<Self>) {
        let Some((path, text, human_ranges)) = self.authorship_payload(cx) else {
            return;
        };

        let background_executor = cx.background_executor().clone();
        self.serialize_authorship = cx.background_spawn(async move {
            background_executor
                .timer(workspace::SERIALIZATION_THROTTLE_TIME)
                .await;
            authorship::save(&path, text, human_ranges).log_err();
        });
    }

    pub(crate) fn persist_authorship_now(&self, cx: &App) {
        let Some((path, text, human_ranges)) = self.authorship_payload(cx) else {
            return;
        };

        authorship::save(&path, text, human_ranges).log_err();
    }

    fn authorship_payload(&self, cx: &App) -> Option<(PathBuf, String, Vec<Range<usize>>)> {
        let (buffer, path) = self.singleton_authorship_buffer_and_path(cx)?;
        let buffer = buffer.read(cx);
        let text = buffer.text_for_range(0..buffer.len()).collect::<String>();
        Some((path, text, buffer.human_authorship_ranges_as_offsets()))
    }

    fn singleton_authorship_buffer_and_path(&self, cx: &App) -> Option<(Entity<Buffer>, PathBuf)> {
        let buffer = self.buffer.read(cx).as_singleton()?.clone();
        let path = {
            let buffer = buffer.read(cx);
            let file = project::File::from_dyn(buffer.file())?;
            if file.is_private {
                return None;
            }
            file.abs_path(cx)
        };
        Some((buffer, path))
    }

    pub fn authorship_visible(&self) -> bool {
        self.show_authorship
    }

    pub fn toggle_authorship(
        &mut self,
        _: &zen_actions::editor::ToggleAuthorship,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_authorship = !self.show_authorship;
        self.refresh_authorship_highlights(cx);
        cx.notify();
    }

    fn refresh_authorship_highlights(&mut self, cx: &mut Context<Self>) {
        if !self.show_authorship {
            self.clear_highlights(HighlightKey::AuthorshipHuman, cx);
            return;
        }

        let snapshot = self.buffer.read(cx).snapshot(cx);
        let ranges = self
            .buffer
            .read(cx)
            .all_buffers()
            .into_iter()
            .flat_map(|buffer| {
                let buffer = buffer.read(cx);
                buffer
                    .human_authorship_ranges()
                    .iter()
                    .filter_map(|range| snapshot.buffer_anchor_range_to_anchor_range(range.clone()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        self.highlight_text(
            HighlightKey::AuthorshipHuman,
            ranges,
            HighlightStyle {
                background_color: Some(cx.theme().status().created_background),
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(cx.theme().status().created),
                    wavy: false,
                }),
                ..Default::default()
            },
            cx,
        );
    }

    pub fn mark_selection_as_human(
        &mut self,
        _: &zen_actions::editor::MarkSelectionAsHuman,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mark_selection_authorship(true, cx);
    }

    pub fn mark_selection_as_agent(
        &mut self,
        _: &zen_actions::editor::MarkSelectionAsAgent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mark_selection_authorship(false, cx);
    }

    fn mark_selection_authorship(&mut self, human: bool, cx: &mut Context<Self>) {
        let snapshot = self.buffer.read(cx).snapshot(cx);
        let selections = self.selections.disjoint_anchors_arc();
        let mut ranges_by_buffer: HashMap<BufferId, Vec<Range<text::Anchor>>> = HashMap::default();

        for selection in selections.iter() {
            if selection.start == selection.end {
                continue;
            }
            if let Some((buffer_snapshot, range)) =
                snapshot.anchor_range_to_buffer_anchor_range(selection.range())
            {
                ranges_by_buffer
                    .entry(buffer_snapshot.remote_id())
                    .or_default()
                    .push(range);
            }
        }

        if ranges_by_buffer.is_empty() {
            return;
        }

        let buffers = self.buffer.read(cx).all_buffers();
        for buffer in buffers {
            let buffer_id = buffer.read(cx).remote_id();
            if let Some(ranges) = ranges_by_buffer.remove(&buffer_id) {
                buffer.update(cx, |buffer, _| {
                    buffer.mark_human_authorship_ranges(ranges, human);
                });
            }
        }

        self.refresh_authorship_highlights(cx);
        self.persist_authorship(cx);
        cx.notify();
    }

    pub fn edit_with_block_indent<I, S, T>(
        &mut self,
        edits: I,
        original_indent_columns: Vec<Option<u32>>,
        cx: &mut Context<Self>,
    ) where
        I: IntoIterator<Item = (Range<S>, T)>,
        S: ToOffset,
        T: Into<Arc<str>>,
    {
        if self.read_only(cx) {
            return;
        }

        self.buffer.update(cx, |buffer, cx| {
            buffer.edit(
                edits,
                Some(AutoindentMode::Block {
                    original_indent_columns,
                }),
                cx,
            )
        });
    }

    fn select(&mut self, phase: SelectPhase, window: &mut Window, cx: &mut Context<Self>) {
        match phase {
            SelectPhase::Begin {
                position,
                add,
                click_count,
            } => self.begin_selection(position, add, click_count, window, cx),
            SelectPhase::BeginColumnar {
                position,
                goal_column,
                reset,
                mode,
            } => self.begin_columnar_selection(position, goal_column, reset, mode, window, cx),
            SelectPhase::Extend {
                position,
                click_count,
            } => self.extend_selection(position, click_count, window, cx),
            SelectPhase::Update {
                position,
                goal_column,
                scroll_delta,
            } => self.update_selection(position, goal_column, scroll_delta, window, cx),
            SelectPhase::End => self.end_selection(window, cx),
        }
    }

    fn extend_selection(
        &mut self,
        position: DisplayPoint,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let tail = self
            .selections
            .newest::<MultiBufferOffset>(&display_map)
            .tail();
        let click_count = click_count.max(match self.selections.select_mode() {
            SelectMode::Character => 1,
            SelectMode::Word(_) => 2,
            SelectMode::Line(_) => 3,
            SelectMode::All => 4,
        });
        self.begin_selection(position, false, click_count, window, cx);

        let tail_anchor = display_map.buffer_snapshot().anchor_before(tail);

        let current_selection = match self.selections.select_mode() {
            SelectMode::Character | SelectMode::All => tail_anchor..tail_anchor,
            SelectMode::Word(range) | SelectMode::Line(range) => range.clone(),
        };

        let mut pending_selection = self
            .selections
            .pending_anchor()
            .cloned()
            .expect("extend_selection not called with pending selection");

        if pending_selection
            .start
            .cmp(&current_selection.start, display_map.buffer_snapshot())
            == Ordering::Greater
        {
            pending_selection.start = current_selection.start;
        }
        if pending_selection
            .end
            .cmp(&current_selection.end, display_map.buffer_snapshot())
            == Ordering::Less
        {
            pending_selection.end = current_selection.end;
            pending_selection.reversed = true;
        }

        let mut pending_mode = self.selections.pending_mode().unwrap();
        match &mut pending_mode {
            SelectMode::Word(range) | SelectMode::Line(range) => *range = current_selection,
            _ => {}
        }

        let effects = if EditorSettings::get_global(cx).autoscroll_on_clicks {
            SelectionEffects::scroll(Autoscroll::fit())
        } else {
            SelectionEffects::no_scroll()
        };

        self.change_selections(effects, window, cx, |s| {
            s.set_pending(pending_selection.clone(), pending_mode);
            s.set_is_extending(true);
        });
    }

    fn begin_selection(
        &mut self,
        position: DisplayPoint,
        add: bool,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus_handle.is_focused(window) {
            self.last_focused_descendant = None;
            window.focus(&self.focus_handle, cx);
        }

        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let buffer = display_map.buffer_snapshot();
        let position = display_map.clip_point(position, Bias::Left);

        let start;
        let end;
        let mode;
        let mut auto_scroll;
        match click_count {
            1 => {
                start = buffer.anchor_before(position.to_point(&display_map));
                end = start;
                mode = SelectMode::Character;
                auto_scroll = true;
            }
            2 => {
                let position = display_map
                    .clip_point(position, Bias::Left)
                    .to_offset(&display_map, Bias::Left);
                let (range, _) = buffer.surrounding_word(position);
                start = buffer.anchor_before(range.start);
                end = buffer.anchor_before(range.end);
                mode = SelectMode::Word(start..end);
                auto_scroll = true;
            }
            3 => {
                let position = display_map
                    .clip_point(position, Bias::Left)
                    .to_point(&display_map);
                let line_start = display_map.prev_line_boundary(position).0;
                let next_line_start = buffer.clip_point(
                    display_map.next_line_boundary(position).0 + Point::new(1, 0),
                    Bias::Left,
                );
                start = buffer.anchor_before(line_start);
                end = buffer.anchor_before(next_line_start);
                mode = SelectMode::Line(start..end);
                auto_scroll = true;
            }
            _ => {
                start = buffer.anchor_before(MultiBufferOffset(0));
                end = buffer.anchor_before(buffer.len());
                mode = SelectMode::All;
                auto_scroll = false;
            }
        }
        auto_scroll &= EditorSettings::get_global(cx).autoscroll_on_clicks;

        let point_to_delete: Option<usize> = {
            let selected_points: Vec<Selection<Point>> =
                self.selections.disjoint_in_range(start..end, &display_map);

            if !add || click_count > 1 {
                None
            } else if !selected_points.is_empty() {
                Some(selected_points[0].id)
            } else {
                let clicked_point_already_selected =
                    self.selections.disjoint_anchors().iter().find(|selection| {
                        selection.start.to_point(buffer) == start.to_point(buffer)
                            || selection.end.to_point(buffer) == end.to_point(buffer)
                    });

                clicked_point_already_selected.map(|selection| selection.id)
            }
        };

        let selections_count = self.selections.count();
        let effects = if auto_scroll {
            SelectionEffects::default()
        } else {
            SelectionEffects::no_scroll()
        };

        self.change_selections(effects, window, cx, |s| {
            if let Some(point_to_delete) = point_to_delete {
                s.delete(point_to_delete);

                if selections_count == 1 {
                    s.set_pending_anchor_range(start..end, mode);
                }
            } else {
                if !add {
                    s.clear_disjoint();
                }

                s.set_pending_anchor_range(start..end, mode);
            }
        });
    }

    fn begin_columnar_selection(
        &mut self,
        position: DisplayPoint,
        goal_column: u32,
        reset: bool,
        mode: ColumnarMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus_handle.is_focused(window) {
            self.last_focused_descendant = None;
            window.focus(&self.focus_handle, cx);
        }

        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));

        if reset {
            let pointer_position = display_map
                .buffer_snapshot()
                .anchor_before(position.to_point(&display_map));

            self.change_selections(
                SelectionEffects::scroll(Autoscroll::newest()),
                window,
                cx,
                |s| {
                    s.clear_disjoint();
                    s.set_pending_anchor_range(
                        pointer_position..pointer_position,
                        SelectMode::Character,
                    );
                },
            );
        };

        let tail = self.selections.newest::<Point>(&display_map).tail();
        let selection_anchor = display_map.buffer_snapshot().anchor_before(tail);
        self.columnar_selection_state = match mode {
            ColumnarMode::FromMouse => Some(ColumnarSelectionState::FromMouse {
                selection_tail: selection_anchor,
                display_point: if reset {
                    if position.column() != goal_column {
                        Some(DisplayPoint::new(position.row(), goal_column))
                    } else {
                        None
                    }
                } else {
                    None
                },
            }),
            ColumnarMode::FromSelection => Some(ColumnarSelectionState::FromSelection {
                selection_tail: selection_anchor,
            }),
        };

        if !reset {
            self.select_columns(position, goal_column, &display_map, window, cx);
        }
    }

    fn update_selection(
        &mut self,
        position: DisplayPoint,
        goal_column: u32,
        scroll_delta: gpui::Point<f32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));

        if self.columnar_selection_state.is_some() {
            self.select_columns(position, goal_column, &display_map, window, cx);
        } else if let Some(mut pending) = self.selections.pending_anchor().cloned() {
            let buffer = display_map.buffer_snapshot();
            let head;
            let tail;
            let mode = self.selections.pending_mode().unwrap();
            match &mode {
                SelectMode::Character => {
                    head = position.to_point(&display_map);
                    tail = pending.tail().to_point(buffer);
                }
                SelectMode::Word(original_range) => {
                    let offset = display_map
                        .clip_point(position, Bias::Left)
                        .to_offset(&display_map, Bias::Left);
                    let original_range = original_range.to_offset(buffer);

                    let head_offset =
                        if buffer.is_inside_word(offset) || original_range.contains(&offset) {
                            let (word_range, _) = buffer.surrounding_word(offset);
                            if word_range.start < original_range.start {
                                word_range.start
                            } else {
                                word_range.end
                            }
                        } else {
                            offset
                        };

                    head = head_offset.to_point(buffer);
                    if head_offset <= original_range.start {
                        tail = original_range.end.to_point(buffer);
                    } else {
                        tail = original_range.start.to_point(buffer);
                    }
                }
                SelectMode::Line(original_range) => {
                    let original_range = original_range.to_point(display_map.buffer_snapshot());

                    let position = display_map
                        .clip_point(position, Bias::Left)
                        .to_point(&display_map);
                    let line_start = display_map.prev_line_boundary(position).0;
                    let next_line_start = buffer.clip_point(
                        display_map.next_line_boundary(position).0 + Point::new(1, 0),
                        Bias::Left,
                    );

                    if line_start < original_range.start {
                        head = line_start
                    } else {
                        head = next_line_start
                    }

                    if head <= original_range.start {
                        tail = original_range.end;
                    } else {
                        tail = original_range.start;
                    }
                }
                SelectMode::All => {
                    return;
                }
            };

            if head < tail {
                pending.start = buffer.anchor_before(head);
                pending.end = buffer.anchor_before(tail);
                pending.reversed = true;
            } else {
                pending.start = buffer.anchor_before(tail);
                pending.end = buffer.anchor_before(head);
                pending.reversed = false;
            }

            self.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                s.set_pending(pending.clone(), mode);
            });
        } else {
            log::error!("update_selection dispatched with no pending selection");
            return;
        }

        self.apply_scroll_delta(scroll_delta, window, cx);
        cx.notify();
    }

    fn end_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.columnar_selection_state.take();
        if let Some(pending_mode) = self.selections.pending_mode() {
            let selections = self
                .selections
                .all::<MultiBufferOffset>(&self.display_snapshot(cx));
            self.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                s.select(selections);
                s.clear_pending();
                if s.is_extending() {
                    s.set_is_extending(false);
                } else {
                    s.set_select_mode(pending_mode);
                }
            });
        }
    }

    fn select_columns(
        &mut self,
        head: DisplayPoint,
        goal_column: u32,
        display_map: &DisplaySnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(columnar_state) = self.columnar_selection_state.as_ref() else {
            return;
        };

        let tail = match columnar_state {
            ColumnarSelectionState::FromMouse {
                selection_tail,
                display_point,
            } => display_point.unwrap_or_else(|| selection_tail.to_display_point(display_map)),
            ColumnarSelectionState::FromSelection { selection_tail } => {
                selection_tail.to_display_point(display_map)
            }
        };

        let start_row = cmp::min(tail.row(), head.row());
        let end_row = cmp::max(tail.row(), head.row());
        let start_column = cmp::min(tail.column(), goal_column);
        let end_column = cmp::max(tail.column(), goal_column);
        let reversed = start_column < tail.column();

        let selection_ranges = (start_row.0..=end_row.0)
            .map(DisplayRow)
            .filter_map(|row| {
                if (matches!(columnar_state, ColumnarSelectionState::FromMouse { .. })
                    || start_column <= display_map.line_len(row))
                    && !display_map.is_block_line(row)
                {
                    let start = display_map
                        .clip_point(DisplayPoint::new(row, start_column), Bias::Left)
                        .to_point(display_map);
                    let end = display_map
                        .clip_point(DisplayPoint::new(row, end_column), Bias::Right)
                        .to_point(display_map);
                    if reversed {
                        Some(end..start)
                    } else {
                        Some(start..end)
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if selection_ranges.is_empty() {
            return;
        }

        let ranges = match columnar_state {
            ColumnarSelectionState::FromMouse { .. } => {
                let mut non_empty_ranges = selection_ranges
                    .iter()
                    .filter(|selection_range| selection_range.start != selection_range.end)
                    .peekable();
                if non_empty_ranges.peek().is_some() {
                    non_empty_ranges.cloned().collect()
                } else {
                    selection_ranges
                }
            }
            _ => selection_ranges,
        };

        self.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges(ranges);
        });
        cx.notify();
    }

    pub fn has_non_empty_selection(&self, snapshot: &DisplaySnapshot) -> bool {
        self.selections
            .all_adjusted(snapshot)
            .iter()
            .any(|selection| !selection.is_empty())
    }

    pub fn has_pending_nonempty_selection(&self) -> bool {
        let pending_nonempty_selection = match self.selections.pending_anchor() {
            Some(Selection { start, end, .. }) => start != end,
            None => false,
        };

        pending_nonempty_selection
            || (self.columnar_selection_state.is_some()
                && self.selections.disjoint_anchors().len() > 1)
    }

    pub fn has_pending_selection(&self) -> bool {
        self.selections.pending_anchor().is_some() || self.columnar_selection_state.is_some()
    }

    pub fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.selection_mark_mode = false;
        self.selection_drag_state = SelectionDragState::None;

        if self.dismiss_menus_and_popups(true, window, cx) {
            cx.notify();
            return;
        }
        if self.mode.is_full()
            && self.change_selections(Default::default(), window, cx, |s| s.try_cancel())
        {
            cx.notify();
            return;
        }

        cx.propagate();
    }

    pub fn dismiss_menus_and_popups(
        &mut self,
        _is_user_requested: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut dismissed = false;

        dismissed |= hide_hover(self, cx);
        dismissed |= self.mouse_context_menu.take().is_some();

        dismissed
    }

    pub fn handle_input(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text: Arc<str> = text.into();

        if self.read_only(cx) {
            return;
        }

        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);

        self.unfold_buffers_with_selections(cx);

        let selections = self.selections.all_adjusted(&self.display_snapshot(cx));
        let mut edits = Vec::new();
        let mut new_selections = Vec::with_capacity(selections.len());
        let mut new_autoclose_regions = Vec::new();
        let snapshot = self.buffer.read(cx).read(cx);
        let mut all_selections_read_only = true;
        let mut has_adjacent_edits = false;
        let mut in_adjacent_group = false;

        let mut regions = self
            .selections_with_autoclose_regions(selections, &snapshot)
            .peekable();

        while let Some((selection, autoclose_region)) = regions.next() {
            if snapshot
                .point_to_buffer_point(selection.head())
                .is_none_or(|(snapshot, ..)| !snapshot.capability.editable())
            {
                continue;
            }
            if snapshot
                .point_to_buffer_point(selection.tail())
                .is_none_or(|(snapshot, ..)| !snapshot.capability.editable())
            {
                // note, ideally we'd clip the tail to the closest writeable region towards the head
                continue;
            }
            all_selections_read_only = false;

            if let Some(scope) = snapshot.language_scope_at(selection.head()) {
                // Determine if the inserted text matches the opening or closing
                // bracket of any of this language's bracket pairs.
                let mut bracket_pair = None;
                let mut is_bracket_pair_start = false;
                let mut is_bracket_pair_end = false;
                if !text.is_empty() {
                    let mut bracket_pair_matching_end = None;
                    // `text` can be empty when a user is using IME (e.g. Chinese Wubi Simplified)
                    //  and they are removing the character that triggered IME popup.
                    for (pair, enabled) in scope.brackets() {
                        if !pair.close && !pair.surround {
                            continue;
                        }

                        if enabled && pair.start.ends_with(text.as_ref()) {
                            let prefix_len = pair.start.len() - text.len();
                            let preceding_text_matches_prefix = prefix_len == 0
                                || (selection.start.column >= (prefix_len as u32)
                                    && snapshot.contains_str_at(
                                        Point::new(
                                            selection.start.row,
                                            selection.start.column - (prefix_len as u32),
                                        ),
                                        &pair.start[..prefix_len],
                                    ));
                            if preceding_text_matches_prefix {
                                bracket_pair = Some(pair.clone());
                                is_bracket_pair_start = true;
                                break;
                            }
                        }
                        if pair.end.as_str() == text.as_ref() && bracket_pair_matching_end.is_none()
                        {
                            // take first bracket pair matching end, but don't break in case a later bracket
                            // pair matches start
                            bracket_pair_matching_end = Some(pair.clone());
                        }
                    }
                    if let Some(end) = bracket_pair_matching_end
                        && bracket_pair.is_none()
                    {
                        bracket_pair = Some(end);
                        is_bracket_pair_end = true;
                    }
                }

                if let Some(bracket_pair) = bracket_pair {
                    let snapshot_settings = snapshot.language_settings_at(selection.start, cx);
                    let autoclose = self.use_autoclose && snapshot_settings.use_autoclose;
                    let auto_surround =
                        self.use_auto_surround && snapshot_settings.use_auto_surround;
                    if selection.is_empty() {
                        if is_bracket_pair_start {
                            // If the inserted text is a suffix of an opening bracket and the
                            // selection is preceded by the rest of the opening bracket, then
                            // insert the closing bracket.
                            let following_text_allows_autoclose = snapshot
                                .chars_at(selection.start)
                                .next()
                                .is_none_or(|c| scope.should_autoclose_before(c));

                            let preceding_text_allows_autoclose = selection.start.column == 0
                                || snapshot
                                    .reversed_chars_at(selection.start)
                                    .next()
                                    .is_none_or(|c| {
                                        bracket_pair.start != bracket_pair.end
                                            || !snapshot
                                                .char_classifier_at(selection.start)
                                                .is_word(c)
                                    });

                            let is_closing_quote = if bracket_pair.end == bracket_pair.start
                                && bracket_pair.start.len() == 1
                            {
                                let target = bracket_pair.start.chars().next().unwrap();
                                let mut byte_offset = 0u32;
                                let current_line_count = snapshot
                                    .reversed_chars_at(selection.start)
                                    .take_while(|&c| c != '\n')
                                    .filter(|c| {
                                        byte_offset += c.len_utf8() as u32;
                                        if *c != target {
                                            return false;
                                        }

                                        let point = Point::new(
                                            selection.start.row,
                                            selection.start.column.saturating_sub(byte_offset),
                                        );

                                        let is_enabled = snapshot
                                            .language_scope_at(point)
                                            .and_then(|scope| {
                                                scope
                                                    .brackets()
                                                    .find(|(pair, _)| {
                                                        pair.start == bracket_pair.start
                                                    })
                                                    .map(|(_, enabled)| enabled)
                                            })
                                            .unwrap_or(true);

                                        let is_delimiter = snapshot
                                            .language_scope_at(Point::new(
                                                point.row,
                                                point.column + 1,
                                            ))
                                            .and_then(|scope| {
                                                scope
                                                    .brackets()
                                                    .find(|(pair, _)| {
                                                        pair.start == bracket_pair.start
                                                    })
                                                    .map(|(_, enabled)| !enabled)
                                            })
                                            .unwrap_or(false);

                                        is_enabled && !is_delimiter
                                    })
                                    .count();
                                current_line_count % 2 == 1
                            } else {
                                false
                            };

                            if autoclose
                                && bracket_pair.close
                                && following_text_allows_autoclose
                                && preceding_text_allows_autoclose
                                && !is_closing_quote
                            {
                                let anchor = snapshot.anchor_before(selection.end);
                                new_selections.push((selection.map(|_| anchor), text.len()));
                                new_autoclose_regions.push((
                                    anchor,
                                    text.len(),
                                    selection.id,
                                    bracket_pair.clone(),
                                ));
                                edits.push((
                                    selection.range(),
                                    format!("{}{}", text, bracket_pair.end).into(),
                                ));
                                continue;
                            }
                        }

                        if let Some(region) = autoclose_region {
                            // If the selection is followed by an auto-inserted closing bracket,
                            // then don't insert that closing bracket again; just move the selection
                            // past the closing bracket.
                            let should_skip = selection.end == region.range.end.to_point(&snapshot)
                                && text.as_ref() == region.pair.end.as_str()
                                && snapshot.contains_str_at(region.range.end, text.as_ref());
                            if should_skip {
                                let anchor = snapshot.anchor_after(selection.end);
                                new_selections
                                    .push((selection.map(|_| anchor), region.pair.end.len()));
                                continue;
                            }
                        }

                        let always_treat_brackets_as_autoclosed = snapshot
                            .language_settings_at(selection.start, cx)
                            .always_treat_brackets_as_autoclosed;
                        if always_treat_brackets_as_autoclosed
                            && is_bracket_pair_end
                            && snapshot.contains_str_at(selection.end, text.as_ref())
                        {
                            // Otherwise, when `always_treat_brackets_as_autoclosed` is set to `true
                            // and the inserted text is a closing bracket and the selection is followed
                            // by the closing bracket then move the selection past the closing bracket.
                            let anchor = snapshot.anchor_after(selection.end);
                            new_selections.push((selection.map(|_| anchor), text.len()));
                            continue;
                        }
                    }
                    // If an opening bracket is 1 character long and is typed while
                    // text is selected, then surround that text with the bracket pair.
                    else if auto_surround
                        && bracket_pair.surround
                        && is_bracket_pair_start
                        && bracket_pair.start.chars().count() == 1
                    {
                        edits.push((selection.start..selection.start, text.clone()));
                        edits.push((
                            selection.end..selection.end,
                            bracket_pair.end.as_str().into(),
                        ));
                        new_selections.push((
                            Selection {
                                id: selection.id,
                                start: snapshot.anchor_after(selection.start),
                                end: snapshot.anchor_before(selection.end),
                                reversed: selection.reversed,
                                goal: selection.goal,
                            },
                            0,
                        ));
                        continue;
                    }
                }
            }

            let next_is_adjacent = regions
                .peek()
                .is_some_and(|(next, _)| selection.end == next.start);

            // If not handling any auto-close operation, then just replace the selected
            // text with the given input and move the selection to the end of the
            // newly inserted text.
            let anchor = if in_adjacent_group || next_is_adjacent {
                // After edits the right bias would shift those anchor to the next visible fragment
                // but we want to resolve to the previous one
                snapshot.anchor_before(selection.end)
            } else {
                snapshot.anchor_after(selection.end)
            };

            new_selections.push((selection.map(|_| anchor), 0));
            edits.push((selection.start..selection.end, text.clone()));

            has_adjacent_edits |= next_is_adjacent;
            in_adjacent_group = next_is_adjacent;
        }

        if all_selections_read_only {
            return;
        }

        drop(regions);
        drop(snapshot);

        self.transact(window, cx, |this, window, cx| {
            this.buffer.update(cx, |buffer, cx| {
                if has_adjacent_edits {
                    buffer.edit_non_coalesce_with_authorship(
                        edits,
                        this.autoindent_mode.clone(),
                        AuthorshipSource::Human,
                        cx,
                    );
                } else {
                    buffer.edit_with_authorship(
                        edits,
                        this.autoindent_mode.clone(),
                        AuthorshipSource::Human,
                        cx,
                    );
                }
            });
            let new_anchor_selections = new_selections.iter().map(|e| &e.0);
            let new_selection_deltas = new_selections.iter().map(|e| e.1);
            let map = this.display_map.update(cx, |map, cx| map.snapshot(cx));
            let new_selections = resolve_selections_wrapping_blocks::<MultiBufferOffset, _>(
                new_anchor_selections,
                &map,
            )
            .zip(new_selection_deltas)
            .map(|(selection, delta)| Selection {
                id: selection.id,
                start: selection.start + delta,
                end: selection.end + delta,
                reversed: selection.reversed,
                goal: SelectionGoal::None,
            })
            .collect::<Vec<_>>();

            let mut i = 0;
            for (position, delta, selection_id, pair) in new_autoclose_regions {
                let position = position.to_offset(map.buffer_snapshot()) + delta;
                let start = map.buffer_snapshot().anchor_before(position);
                let end = map.buffer_snapshot().anchor_after(position);
                while let Some(existing_state) = this.autoclose_regions.get(i) {
                    match existing_state
                        .range
                        .start
                        .cmp(&start, map.buffer_snapshot())
                    {
                        Ordering::Less => i += 1,
                        Ordering::Greater => break,
                        Ordering::Equal => {
                            match end.cmp(&existing_state.range.end, map.buffer_snapshot()) {
                                Ordering::Less => i += 1,
                                Ordering::Equal => break,
                                Ordering::Greater => break,
                            }
                        }
                    }
                }
                this.autoclose_regions.insert(
                    i,
                    AutocloseRegion {
                        selection_id,
                        range: start..end,
                        pair,
                    },
                );
            }

            this.change_selections(
                SelectionEffects::scroll(Autoscroll::fit()),
                window,
                cx,
                |s| s.select(new_selections),
            );

            this.refresh_edit_prediction(true, false, window, cx);
        });
    }

    pub fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }

        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        self.transact(window, cx, |this, window, cx| {
            let (edits_with_flags, selection_info): (Vec<_>, Vec<_>) = {
                let selections = this
                    .selections
                    .all::<MultiBufferOffset>(&this.display_snapshot(cx));
                let multi_buffer = this.buffer.read(cx);
                let buffer = multi_buffer.snapshot(cx);
                selections
                    .iter()
                    .map(|selection| {
                        let start_point = selection.start.to_point(&buffer);
                        let mut existing_indent =
                            buffer.indent_size_for_line(MultiBufferRow(start_point.row));
                        let full_indent_len = existing_indent.len;
                        existing_indent.len = cmp::min(existing_indent.len, start_point.column);
                        let mut start = selection.start;
                        let end = selection.end;
                        let selection_is_empty = start == end;
                        let language_scope = buffer.language_scope_at(start);
                        let (delimiter, newline_config) = if let Some(language) = &language_scope {
                            let needs_extra_newline = NewlineConfig::insert_extra_newline_brackets(
                                &buffer,
                                start..end,
                                language,
                            )
                                || NewlineConfig::insert_extra_newline_tree_sitter(
                                    &buffer,
                                    start..end,
                                );

                            let mut newline_config = NewlineConfig::Newline {
                                additional_indent: IndentSize::spaces(0),
                                extra_line_additional_indent: if needs_extra_newline {
                                    Some(IndentSize::spaces(0))
                                } else {
                                    None
                                },
                                prevent_auto_indent: false,
                            };

                            let comment_delimiter = maybe!({
                                if !selection_is_empty {
                                    return None;
                                }

                                if !multi_buffer.language_settings(cx).extend_comment_on_newline {
                                    return None;
                                }

                                return comment_delimiter_for_newline(
                                    &start_point,
                                    &buffer,
                                    language,
                                );
                            });

                            let doc_delimiter = maybe!({
                                if !selection_is_empty {
                                    return None;
                                }

                                if !multi_buffer.language_settings(cx).extend_comment_on_newline {
                                    return None;
                                }

                                return documentation_delimiter_for_newline(
                                    &start_point,
                                    &buffer,
                                    language,
                                    &mut newline_config,
                                );
                            });

                            let list_delimiter = maybe!({
                                if !selection_is_empty {
                                    return None;
                                }

                                if !multi_buffer.language_settings(cx).extend_list_on_newline {
                                    return None;
                                }

                                return list_delimiter_for_newline(
                                    &start_point,
                                    &buffer,
                                    language,
                                    &mut newline_config,
                                );
                            });

                            (
                                comment_delimiter.or(doc_delimiter).or(list_delimiter),
                                newline_config,
                            )
                        } else {
                            (
                                None,
                                NewlineConfig::Newline {
                                    additional_indent: IndentSize::spaces(0),
                                    extra_line_additional_indent: None,
                                    prevent_auto_indent: false,
                                },
                            )
                        };

                        let (edit_start, new_text, prevent_auto_indent) = match &newline_config {
                            NewlineConfig::ClearCurrentLine => {
                                let row_start =
                                    buffer.point_to_offset(Point::new(start_point.row, 0));
                                (row_start, String::new(), false)
                            }
                            NewlineConfig::UnindentCurrentLine { continuation } => {
                                let row_start =
                                    buffer.point_to_offset(Point::new(start_point.row, 0));
                                let tab_size = buffer.language_settings_at(start, cx).tab_size;
                                let tab_size_indent = IndentSize::spaces(tab_size.get());
                                let reduced_indent =
                                    existing_indent.with_delta(Ordering::Less, tab_size_indent);
                                let mut new_text = String::new();
                                new_text.extend(reduced_indent.chars());
                                new_text.push_str(continuation);
                                (row_start, new_text, true)
                            }
                            NewlineConfig::Newline {
                                additional_indent,
                                extra_line_additional_indent,
                                prevent_auto_indent,
                            } => {
                                let auto_indent_mode =
                                    buffer.language_settings_at(start, cx).auto_indent;
                                let preserve_indent =
                                    auto_indent_mode != language::AutoIndentMode::None;
                                let apply_syntax_indent =
                                    auto_indent_mode == language::AutoIndentMode::SyntaxAware;
                                let capacity_for_delimiter =
                                    delimiter.as_deref().map(str::len).unwrap_or_default();
                                let existing_indent_len = if preserve_indent {
                                    existing_indent.len as usize
                                } else {
                                    0
                                };
                                let extra_line_len = extra_line_additional_indent
                                    .map(|i| 1 + existing_indent_len + i.len as usize)
                                    .unwrap_or(0);
                                let mut new_text = String::with_capacity(
                                    1 + capacity_for_delimiter
                                        + existing_indent_len
                                        + additional_indent.len as usize
                                        + extra_line_len,
                                );
                                new_text.push('\n');
                                if preserve_indent {
                                    new_text.extend(existing_indent.chars());
                                }
                                new_text.extend(additional_indent.chars());
                                if let Some(delimiter) = &delimiter {
                                    new_text.push_str(delimiter);
                                }
                                if let Some(extra_indent) = extra_line_additional_indent {
                                    new_text.push('\n');
                                    if preserve_indent {
                                        new_text.extend(existing_indent.chars());
                                    }
                                    new_text.extend(extra_indent.chars());
                                }
                                // Extend the edit to the beginning of the line
                                // to clear auto-indent whitespace that would
                                // otherwise remain as trailing whitespace. This
                                // applies to blank lines and lines where only
                                // indentation remains before the cursor.
                                if selection_is_empty
                                    && preserve_indent
                                    && full_indent_len > 0
                                    && start_point.column == full_indent_len
                                {
                                    start = buffer.point_to_offset(Point::new(start_point.row, 0));
                                }

                                (
                                    start,
                                    new_text,
                                    *prevent_auto_indent || !apply_syntax_indent,
                                )
                            }
                        };

                        let anchor = buffer.anchor_after(end);
                        let new_selection = selection.map(|_| anchor);
                        (
                            ((edit_start..end, new_text), prevent_auto_indent),
                            (newline_config.has_extra_line(), new_selection),
                        )
                    })
                    .unzip()
            };

            let mut auto_indent_edits = Vec::new();
            let mut edits = Vec::new();
            for (edit, prevent_auto_indent) in edits_with_flags {
                if prevent_auto_indent {
                    edits.push(edit);
                } else {
                    auto_indent_edits.push(edit);
                }
            }
            if !edits.is_empty() {
                this.edit_with_authorship(edits, AuthorshipSource::Human, cx);
            }
            if !auto_indent_edits.is_empty() {
                this.edit_with_autoindent_and_authorship(
                    auto_indent_edits,
                    AuthorshipSource::Human,
                    cx,
                );
            }

            let buffer = this.buffer.read(cx).snapshot(cx);
            let new_selections = selection_info
                .into_iter()
                .map(|(extra_newline_inserted, new_selection)| {
                    let mut cursor = new_selection.end.to_point(&buffer);
                    if extra_newline_inserted {
                        cursor.row -= 1;
                        cursor.column = buffer.line_len(MultiBufferRow(cursor.row));
                    }
                    new_selection.map(|_| cursor)
                })
                .collect();

            this.change_selections(Default::default(), window, cx, |s| s.select(new_selections));
            this.refresh_edit_prediction(true, false, window, cx);
        });
    }

    pub fn newline_above(&mut self, _: &NewlineAbove, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }

        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);

        let buffer = self.buffer.read(cx);
        let snapshot = buffer.snapshot(cx);

        let mut edits = Vec::new();
        let mut rows = Vec::new();

        for (rows_inserted, selection) in self
            .selections
            .all_adjusted(&self.display_snapshot(cx))
            .into_iter()
            .enumerate()
        {
            let cursor = selection.head();
            let row = cursor.row;

            let start_of_line = snapshot.clip_point(Point::new(row, 0), Bias::Left);

            let newline = "\n".to_string();
            edits.push((start_of_line..start_of_line, newline));

            rows.push(row + rows_inserted as u32);
        }

        self.transact(window, cx, |editor, window, cx| {
            editor.edit_with_authorship(edits, AuthorshipSource::Human, cx);

            editor.change_selections(Default::default(), window, cx, |s| {
                let mut index = 0;
                s.move_cursors_with(&mut |map, _, _| {
                    let row = rows[index];
                    index += 1;

                    let point = Point::new(row, 0);
                    let boundary = map.next_line_boundary(point).1;
                    let clipped = map.clip_point(boundary, Bias::Left);

                    (clipped, SelectionGoal::None)
                });
            });

            let mut indent_edits = Vec::new();
            let multibuffer_snapshot = editor.buffer.read(cx).snapshot(cx);
            for row in rows {
                let indents = multibuffer_snapshot.suggested_indents(row..row + 1, cx);
                for (row, indent) in indents {
                    if indent.len == 0 {
                        continue;
                    }

                    let text = match indent.kind {
                        IndentKind::Space => " ".repeat(indent.len as usize),
                        IndentKind::Tab => "\t".repeat(indent.len as usize),
                    };
                    let point = Point::new(row.0, 0);
                    indent_edits.push((point..point, text));
                }
            }
            editor.edit_with_authorship(indent_edits, AuthorshipSource::Human, cx);
        });
    }

    pub fn newline_below(&mut self, _: &NewlineBelow, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }

        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);

        let mut buffer_edits: HashMap<EntityId, (Entity<Buffer>, Vec<Point>)> = HashMap::default();
        let mut rows = Vec::new();
        let mut rows_inserted = 0;

        for selection in self.selections.all_adjusted(&self.display_snapshot(cx)) {
            let cursor = selection.head();
            let row = cursor.row;

            let point = Point::new(row, 0);
            let Some((buffer_handle, buffer_point)) =
                self.buffer.read(cx).point_to_buffer_point(point, cx)
            else {
                continue;
            };

            buffer_edits
                .entry(buffer_handle.entity_id())
                .or_insert_with(|| (buffer_handle, Vec::new()))
                .1
                .push(buffer_point);

            rows_inserted += 1;
            rows.push(row + rows_inserted);
        }

        self.transact(window, cx, |editor, window, cx| {
            for (_, (buffer_handle, points)) in &buffer_edits {
                buffer_handle.update(cx, |buffer, cx| {
                    let edits: Vec<_> = points
                        .iter()
                        .map(|point| {
                            let target = Point::new(point.row + 1, 0);
                            let start_of_line = buffer.point_to_offset(target).min(buffer.len());
                            (start_of_line..start_of_line, "\n")
                        })
                        .collect();
                    buffer.edit_with_authorship(edits, None, AuthorshipSource::Human, cx);
                });
            }

            editor.change_selections(Default::default(), window, cx, |s| {
                let mut index = 0;
                s.move_cursors_with(&mut |map, _, _| {
                    let row = rows[index];
                    index += 1;

                    let point = Point::new(row, 0);
                    let boundary = map.next_line_boundary(point).1;
                    let clipped = map.clip_point(boundary, Bias::Left);

                    (clipped, SelectionGoal::None)
                });
            });

            let mut indent_edits = Vec::new();
            let multibuffer_snapshot = editor.buffer.read(cx).snapshot(cx);
            for row in rows {
                let indents = multibuffer_snapshot.suggested_indents(row..row + 1, cx);
                for (row, indent) in indents {
                    if indent.len == 0 {
                        continue;
                    }

                    let text = match indent.kind {
                        IndentKind::Space => " ".repeat(indent.len as usize),
                        IndentKind::Tab => "\t".repeat(indent.len as usize),
                    };
                    let point = Point::new(row.0, 0);
                    indent_edits.push((point..point, text));
                }
            }
            editor.edit_with_authorship(indent_edits, AuthorshipSource::Human, cx);
        });
    }

    pub fn insert(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let autoindent = text.is_empty().not().then(|| AutoindentMode::Block {
            original_indent_columns: Vec::new(),
        });
        self.replace_selections(text, autoindent, window, cx);
    }

    /// Replaces the editor's selections with the provided `text`, applying the
    /// given `autoindent_mode` (`None` will skip autoindentation).
    ///
    /// Early returns if the editor is in read-only mode, without applying any
    /// edits.
    fn replace_selections(
        &mut self,
        text: &str,
        autoindent_mode: Option<AutoindentMode>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_selections_with_authorship(
            text,
            autoindent_mode,
            AuthorshipSource::Human,
            window,
            cx,
        );
    }

    fn replace_selections_with_authorship(
        &mut self,
        text: &str,
        autoindent_mode: Option<AutoindentMode>,
        authorship_source: AuthorshipSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only(cx) {
            return;
        }

        let text: Arc<str> = text.into();
        self.transact(window, cx, |this, window, cx| {
            let old_selections = this.selections.all_adjusted(&this.display_snapshot(cx));

            let selection_anchors = this.buffer.update(cx, |buffer, cx| {
                let anchors = {
                    let snapshot = buffer.read(cx);
                    old_selections
                        .iter()
                        .map(|s| {
                            let anchor = snapshot.anchor_after(s.head());
                            s.map(|_| anchor)
                        })
                        .collect::<Vec<_>>()
                };
                buffer.edit_with_authorship(
                    old_selections
                        .iter()
                        .map(|s| (s.start..s.end, text.clone())),
                    autoindent_mode,
                    authorship_source,
                    cx,
                );
                anchors
            });

            this.change_selections(Default::default(), window, cx, |s| {
                s.select_anchors(selection_anchors);
            });

            cx.notify();
        });
    }

    /// If any empty selections is touching the start of its innermost containing autoclose
    /// region, expand it to select the brackets.
    fn select_autoclose_pair(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selections = self
            .selections
            .all::<MultiBufferOffset>(&self.display_snapshot(cx));
        let buffer = self.buffer.read(cx).read(cx);
        let new_selections = self
            .selections_with_autoclose_regions(selections, &buffer)
            .map(|(mut selection, region)| {
                if !selection.is_empty() {
                    return selection;
                }

                if let Some(region) = region {
                    let mut range = region.range.to_offset(&buffer);
                    if selection.start == range.start && range.start.0 >= region.pair.start.len() {
                        range.start -= region.pair.start.len();
                        if buffer.contains_str_at(range.start, &region.pair.start)
                            && buffer.contains_str_at(range.end, &region.pair.end)
                        {
                            range.end += region.pair.end.len();
                            selection.start = range.start;
                            selection.end = range.end;

                            return selection;
                        }
                    }
                }

                let always_treat_brackets_as_autoclosed = buffer
                    .language_settings_at(selection.start, cx)
                    .always_treat_brackets_as_autoclosed;

                if !always_treat_brackets_as_autoclosed {
                    return selection;
                }

                if let Some(scope) = buffer.language_scope_at(selection.start) {
                    for (pair, enabled) in scope.brackets() {
                        if !enabled || !pair.close {
                            continue;
                        }

                        if buffer.contains_str_at(selection.start, &pair.end) {
                            let pair_start_len = pair.start.len();
                            if buffer.contains_str_at(
                                selection.start.saturating_sub_usize(pair_start_len),
                                &pair.start,
                            ) {
                                selection.start -= pair_start_len;
                                selection.end += pair.end.len();

                                return selection;
                            }
                        }
                    }
                }

                selection
            })
            .collect();

        drop(buffer);
        self.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
            selections.select(new_selections)
        });
    }

    /// Iterate the given selections, and for each one, find the smallest surrounding
    /// autoclose region. This uses the ordering of the selections and the autoclose
    /// regions to avoid repeated comparisons.
    fn selections_with_autoclose_regions<'a, D: ToOffset + Clone>(
        &'a self,
        selections: impl IntoIterator<Item = Selection<D>>,
        buffer: &'a MultiBufferSnapshot,
    ) -> impl Iterator<Item = (Selection<D>, Option<&'a AutocloseRegion>)> {
        let mut i = 0;
        let mut regions = self.autoclose_regions.as_slice();
        selections.into_iter().map(move |selection| {
            let range = selection.start.to_offset(buffer)..selection.end.to_offset(buffer);

            let mut enclosing = None;
            while let Some(pair_state) = regions.get(i) {
                if pair_state.range.end.to_offset(buffer) < range.start {
                    regions = &regions[i + 1..];
                    i = 0;
                } else if pair_state.range.start.to_offset(buffer) > range.end {
                    break;
                } else {
                    if pair_state.selection_id == selection.id {
                        enclosing = Some(pair_state);
                    }
                    i += 1;
                }
            }

            (selection, enclosing)
        })
    }

    /// Remove any autoclose regions that no longer contain their selection or have invalid anchors in ranges.
    fn invalidate_autoclose_regions(
        &mut self,
        mut selections: &[Selection<Anchor>],
        buffer: &MultiBufferSnapshot,
    ) {
        self.autoclose_regions.retain(|state| {
            if !state.range.start.is_valid(buffer) || !state.range.end.is_valid(buffer) {
                return false;
            }

            let mut i = 0;
            while let Some(selection) = selections.get(i) {
                if selection.end.cmp(&state.range.start, buffer).is_lt() {
                    selections = &selections[1..];
                    continue;
                }
                if selection.start.cmp(&state.range.end, buffer).is_gt() {
                    break;
                }
                if selection.id == state.selection_id {
                    return true;
                } else {
                    i += 1;
                }
            }
            false
        });
    }

    pub fn visible_buffers(&self, cx: &mut Context<Editor>) -> Vec<Entity<Buffer>> {
        let display_snapshot = self.display_snapshot(cx);
        let visible_range = self.multi_buffer_visible_range(&display_snapshot, cx);
        let multi_buffer = self.buffer().read(cx);
        display_snapshot
            .buffer_snapshot()
            .range_to_buffer_ranges(visible_range)
            .into_iter()
            .filter(|(_, excerpt_visible_range, _)| !excerpt_visible_range.is_empty())
            .filter_map(|(buffer_snapshot, _, _)| multi_buffer.buffer(buffer_snapshot.remote_id()))
            .collect()
    }

    pub fn visible_buffer_ranges(
        &self,
        cx: &mut Context<Editor>,
    ) -> Vec<(
        BufferSnapshot,
        Range<BufferOffset>,
        ExcerptRange<text::Anchor>,
    )> {
        let display_snapshot = self.display_snapshot(cx);
        let visible_range = self.multi_buffer_visible_range(&display_snapshot, cx);
        display_snapshot
            .buffer_snapshot()
            .range_to_buffer_ranges(visible_range)
            .into_iter()
            .filter(|(_, excerpt_visible_range, _)| !excerpt_visible_range.is_empty())
            .collect()
    }

    pub fn text_layout_details(&self, window: &mut Window, cx: &mut App) -> TextLayoutDetails {
        TextLayoutDetails {
            text_system: window.text_system().clone(),
            editor_style: self.style.clone().unwrap(),
            rem_size: window.rem_size(),
            scroll_anchor: self.scroll_manager.shared_scroll_anchor(cx),
            visible_rows: self.visible_line_count(),
            vertical_scroll_margin: self.scroll_manager.vertical_scroll_margin,
        }
    }

    fn open_transaction_for_hidden_buffers(
        workspace: Entity<Workspace>,
        transaction: ProjectTransaction,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if transaction.0.is_empty() {
            return;
        }

        let edited_buffers_already_open = {
            let other_editors: Vec<Entity<Editor>> = workspace
                .read(cx)
                .panes()
                .iter()
                .flat_map(|pane| pane.read(cx).items_of_type::<Editor>())
                .filter(|editor| editor.entity_id() != cx.entity_id())
                .collect();

            transaction.0.keys().all(|buffer| {
                other_editors.iter().any(|editor| {
                    let multi_buffer = editor.read(cx).buffer();
                    multi_buffer.read(cx).is_singleton()
                        && multi_buffer
                            .read(cx)
                            .as_singleton()
                            .is_some_and(|singleton| singleton.entity_id() == buffer.entity_id())
                })
            })
        };
        if !edited_buffers_already_open {
            let workspace = workspace.downgrade();
            cx.defer_in(window, move |_, window, cx| {
                cx.spawn_in(window, async move |editor, cx| {
                    Self::open_project_transaction(&editor, workspace, transaction, title, cx)
                        .await
                        .log_err();
                })
                .detach();
            });
        }
    }

    pub async fn open_project_transaction(
        editor: &WeakEntity<Editor>,
        workspace: WeakEntity<Workspace>,
        transaction: ProjectTransaction,
        title: String,
        cx: &mut AsyncWindowContext,
    ) -> Result<()> {
        let mut entries = transaction.0.into_iter().collect::<Vec<_>>();
        cx.update(|_, cx| {
            entries.sort_unstable_by_key(|(buffer, _)| {
                buffer.read(cx).file().map(|file| file.path().clone())
            });
        })?;
        if entries.is_empty() {
            return Ok(());
        }

        if let [(buffer, transaction)] = &*entries {
            let cursor_excerpt = editor.update(cx, |editor, cx| {
                let snapshot = editor.buffer().read(cx).snapshot(cx);
                let head = editor.selections.newest_anchor().head();
                let (buffer_snapshot, excerpt_range) = snapshot.excerpt_containing(head..head)?;
                if buffer_snapshot.remote_id() != buffer.read(cx).remote_id() {
                    return None;
                }
                Some(excerpt_range)
            })?;

            if let Some(excerpt_range) = cursor_excerpt {
                let all_edits_within_excerpt = buffer.read_with(cx, |buffer, _| {
                    let excerpt_range = excerpt_range.context.to_offset(buffer);
                    buffer
                        .edited_ranges_for_transaction::<usize>(transaction)
                        .all(|range| {
                            excerpt_range.start <= range.start && excerpt_range.end >= range.end
                        })
                });

                if all_edits_within_excerpt {
                    return Ok(());
                }
            }
        }

        let mut ranges_to_highlight = Vec::new();
        let excerpt_buffer = cx.new(|cx| {
            let mut multi_buffer = MultiBuffer::new(Capability::ReadWrite).with_title(title);
            for (buffer_handle, transaction) in &entries {
                let edited_ranges = buffer_handle
                    .read(cx)
                    .edited_ranges_for_transaction::<Point>(transaction)
                    .collect::<Vec<_>>();
                multi_buffer.set_excerpts_for_path(
                    PathKey::for_buffer(buffer_handle, cx),
                    buffer_handle.clone(),
                    edited_ranges.clone(),
                    multibuffer_context_lines(cx),
                    cx,
                );
                let snapshot = multi_buffer.snapshot(cx);
                let buffer_snapshot = buffer_handle.read(cx).snapshot();
                ranges_to_highlight.extend(edited_ranges.into_iter().filter_map(|range| {
                    let text_range = buffer_snapshot.anchor_range_inside(range);
                    let start = snapshot.anchor_in_buffer(text_range.start)?;
                    let end = snapshot.anchor_in_buffer(text_range.end)?;
                    Some(start..end)
                }));
            }
            multi_buffer.push_transaction(
                entries
                    .iter()
                    .map(|(buffer, transaction)| (buffer, transaction)),
                cx,
            );
            multi_buffer
        });

        workspace.update_in(cx, |workspace, window, cx| {
            let project = workspace.project().clone();
            let editor =
                cx.new(|cx| Editor::for_multibuffer(excerpt_buffer, Some(project), window, cx));
            workspace.add_item_to_active_pane(Box::new(editor.clone()), None, true, window, cx);
            editor.update(cx, |editor, cx| {
                editor.highlight_background(
                    HighlightKey::Editor,
                    &ranges_to_highlight,
                    |_, theme| theme.colors().editor_highlighted_line_background,
                    cx,
                );
            });
        })?;

        Ok(())
    }

    pub fn has_mouse_context_menu(&self) -> bool {
        self.mouse_context_menu.is_some()
    }

    fn refresh_single_line_folds(&mut self, window: &mut Window, cx: &mut Context<Editor>) {
        struct NewlineFold;
        let type_id = std::any::TypeId::of::<NewlineFold>();
        if !self.mode.is_single_line() {
            return;
        }
        let snapshot = self.snapshot(window, cx);
        if snapshot.buffer_snapshot().max_point().row == 0 {
            return;
        }
        let task = cx.background_spawn(async move {
            let new_newlines = snapshot
                .buffer_chars_at(MultiBufferOffset(0))
                .filter_map(|(character, index)| {
                    if character == '\n' {
                        Some(
                            snapshot.buffer_snapshot().anchor_after(index)
                                ..snapshot.buffer_snapshot().anchor_before(index + 1usize),
                        )
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let existing_newlines = snapshot
                .folds_in_range(MultiBufferOffset(0)..snapshot.buffer_snapshot().len())
                .filter_map(|fold| {
                    if fold.placeholder.type_tag == Some(type_id) {
                        Some(fold.range.start..fold.range.end)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            (new_newlines, existing_newlines)
        });
        self.folding_newlines = cx.spawn(async move |this, cx| {
            let (new_newlines, existing_newlines) = task.await;
            if new_newlines == existing_newlines {
                return;
            }
            let placeholder = FoldPlaceholder {
                render: Arc::new(move |_, _, cx| {
                    div()
                        .bg(cx.theme().status().hint_background)
                        .border_b_1()
                        .size_full()
                        .font(ThemeSettings::get_global(cx).buffer_font.clone())
                        .border_color(cx.theme().status().hint)
                        .child("\\n")
                        .into_any()
                }),
                constrain_width: false,
                merge_adjacent: false,
                type_tag: Some(type_id),
                collapsed_text: None,
            };
            let creases = new_newlines
                .into_iter()
                .map(|range| Crease::simple(range, placeholder.clone()))
                .collect();
            this.update(cx, |this, cx| {
                this.display_map.update(cx, |display_map, cx| {
                    display_map.remove_folds_with_type(existing_newlines, type_id, cx);
                    display_map.fold(creases, cx);
                });
            })
            .log_err();
        });
    }

    fn refresh_selected_text_highlights(
        &mut self,
        _snapshot: &DisplaySnapshot,
        _on_buffer_edit: bool,
        _window: &mut Window,
        _cx: &mut Context<Editor>,
    ) {
    }

    pub fn multi_buffer_visible_range(
        &self,
        display_snapshot: &DisplaySnapshot,
        cx: &App,
    ) -> Range<Point> {
        let visible_start = self
            .scroll_manager
            .native_anchor(display_snapshot, cx)
            .anchor
            .to_point(display_snapshot.buffer_snapshot())
            .to_display_point(display_snapshot);

        let mut target_end = visible_start;
        *target_end.row_mut() += self.visible_line_count().unwrap_or(0.).ceil() as u32;

        visible_start.to_point(display_snapshot)
            ..display_snapshot
                .clip_point(target_end, Bias::Right)
                .to_point(display_snapshot)
    }

    pub fn refresh_edit_prediction(
        &mut self,
        _debounce: bool,
        _user_requested: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<()> {
        None
    }

    fn edit_prediction_requires_modifier(&self) -> bool {
        false
    }

    pub fn has_active_edit_prediction(&self) -> bool {
        false
    }

    pub fn edit_prediction_visible_in_cursor_popover(&self, _has_completion: bool) -> bool {
        false
    }

    fn handle_modifiers_changed(
        &mut self,
        modifiers: Modifiers,
        position_map: &PositionMap,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_selection_mode(&modifiers, position_map, window, cx);

        let mouse_position = window.mouse_position();
        if !position_map.text_hitbox.is_hovered(window) {
            return;
        }

        self.update_hovered_link(
            position_map.point_for_position(mouse_position),
            Some(mouse_position),
            &position_map.snapshot,
            modifiers,
            window,
            cx,
        )
    }

    fn is_cmd_or_ctrl_pressed(modifiers: &Modifiers, cx: &mut Context<Self>) -> bool {
        match EditorSettings::get_global(cx).multi_cursor_modifier {
            MultiCursorModifier::Alt => modifiers.secondary(),
            MultiCursorModifier::CmdOrCtrl => modifiers.alt,
        }
    }

    fn is_alt_pressed(modifiers: &Modifiers, cx: &mut Context<Self>) -> bool {
        match EditorSettings::get_global(cx).multi_cursor_modifier {
            MultiCursorModifier::Alt => modifiers.alt,
            MultiCursorModifier::CmdOrCtrl => modifiers.secondary(),
        }
    }

    fn columnar_selection_mode(
        modifiers: &Modifiers,
        cx: &mut Context<Self>,
    ) -> Option<ColumnarMode> {
        if modifiers.shift && modifiers.number_of_modifiers() == 2 {
            if Self::is_cmd_or_ctrl_pressed(modifiers, cx) {
                Some(ColumnarMode::FromMouse)
            } else if Self::is_alt_pressed(modifiers, cx) {
                Some(ColumnarMode::FromSelection)
            } else {
                None
            }
        } else {
            None
        }
    }

    fn update_selection_mode(
        &mut self,
        modifiers: &Modifiers,
        position_map: &PositionMap,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mode) = Self::columnar_selection_mode(modifiers, cx) else {
            return;
        };
        if self.selections.pending_anchor().is_none() {
            return;
        }

        let mouse_position = window.mouse_position();
        let point_for_position = position_map.point_for_position(mouse_position);
        let position = point_for_position.previous_valid;

        self.select(
            SelectPhase::BeginColumnar {
                position,
                reset: false,
                mode,
                goal_column: point_for_position.exact_unclipped.column(),
            },
            window,
            cx,
        );
    }

    fn update_visible_edit_prediction(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<()> {
        None
    }

    fn render_edit_prediction_popover(
        &mut self,
        _text_bounds: &Bounds<Pixels>,
        _content_origin: gpui::Point<Pixels>,
        _right_margin: Pixels,
        _editor_snapshot: &EditorSnapshot,
        _visible_row_range: Range<DisplayRow>,
        _scroll_top: ScrollOffset,
        _scroll_bottom: ScrollOffset,
        _line_layouts: &[LineWithInvisibles],
        _line_height: Pixels,
        _scroll_position: gpui::Point<ScrollOffset>,
        _scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        _newest_selection_head: Option<DisplayPoint>,
        _editor_width: Pixels,
        _style: &EditorStyle,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<(AnyElement, gpui::Point<Pixels>)> {
        None
    }

    fn edit_prediction_cursor_popover_height(&self) -> Pixels {
        Pixels::ZERO
    }

    fn current_user_player_color(&self, cx: &mut App) -> PlayerColor {
        if self.read_only(cx) {
            cx.theme().players().read_only()
        } else if let Some(style) = self.style.as_ref() {
            style.local_player
        } else {
            cx.theme().players().local()
        }
    }

    fn render_edit_prediction_cursor_popover(
        &self,
        _min_width: Pixels,
        _max_width: Pixels,
        _cursor_point: Point,
        _style: &EditorStyle,
        _window: &mut Window,
        _cx: &mut Context<Editor>,
    ) -> Option<AnyElement> {
        None
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.transact(window, cx, |this, window, cx| {
            this.select_all(&SelectAll, window, cx);
            this.insert("", window, cx);
        });
    }

    pub fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }
        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        self.transact(window, cx, |this, window, cx| {
            this.select_autoclose_pair(window, cx);

            let display_map = this.display_map.update(cx, |map, cx| map.snapshot(cx));
            let mut selections = this.selections.all::<MultiBufferPoint>(&display_map);
            for selection in &mut selections {
                if selection.is_empty() {
                    let old_head = selection.head();
                    let mut new_head =
                        movement::left(&display_map, old_head.to_display_point(&display_map))
                            .to_point(&display_map);
                    if let Some((buffer, line_buffer_range)) = display_map
                        .buffer_snapshot()
                        .buffer_line_for_row(MultiBufferRow(old_head.row))
                    {
                        let indent_size = buffer.indent_size_for_line(line_buffer_range.start.row);
                        let indent_len = match indent_size.kind {
                            IndentKind::Space => {
                                buffer.settings_at(line_buffer_range.start, cx).tab_size
                            }
                            IndentKind::Tab => NonZeroU32::new(1).unwrap(),
                        };
                        if old_head.column <= indent_size.len && old_head.column > 0 {
                            let indent_len = indent_len.get();
                            new_head = cmp::min(
                                new_head,
                                MultiBufferPoint::new(
                                    old_head.row,
                                    ((old_head.column - 1) / indent_len) * indent_len,
                                ),
                            );
                        }
                    }

                    selection.set_head(new_head, SelectionGoal::None);
                }
            }

            this.change_selections(Default::default(), window, cx, |s| s.select(selections));
            this.insert("", window, cx);
            this.refresh_edit_prediction(true, false, window, cx);
        });
    }

    pub fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }
        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        self.transact(window, cx, |this, window, cx| {
            this.change_selections(Default::default(), window, cx, |s| {
                s.move_with(&mut |map, selection| {
                    if selection.is_empty() {
                        let cursor = movement::right(map, selection.head());
                        selection.end = cursor;
                        selection.reversed = true;
                        selection.goal = SelectionGoal::None;
                    }
                })
            });
            this.insert("", window, cx);
            this.refresh_edit_prediction(true, false, window, cx);
        });
    }

    pub fn backtab(&mut self, _: &Backtab, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        self.outdent(&Outdent, window, cx);
    }

    pub fn tab(&mut self, _: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        if self.read_only(cx) {
            return;
        }
        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        let mut selections = self.selections.all_adjusted(&self.display_snapshot(cx));
        let buffer = self.buffer.read(cx);
        let snapshot = buffer.snapshot(cx);
        let rows_iter = selections.iter().map(|s| s.head().row);
        let suggested_indents = snapshot.suggested_indents(rows_iter, cx);

        let has_some_cursor_in_whitespace = selections
            .iter()
            .filter(|selection| selection.is_empty())
            .any(|selection| {
                let cursor = selection.head();
                let current_indent = snapshot.indent_size_for_line(MultiBufferRow(cursor.row));
                cursor.column < current_indent.len
            });

        let mut edits = Vec::new();
        let mut prev_edited_row = 0;
        let mut row_delta = 0;
        for selection in &mut selections {
            if selection.start.row != prev_edited_row {
                row_delta = 0;
            }
            prev_edited_row = selection.end.row;

            // If cursor is after a list prefix, make selection non-empty to trigger line indent
            if selection.is_empty() {
                let cursor = selection.head();
                let settings = buffer.language_settings_at(cursor, cx);
                if settings.indent_list_on_tab {
                    if let Some(language) = snapshot.language_scope_at(Point::new(cursor.row, 0)) {
                        if is_list_prefix_row(MultiBufferRow(cursor.row), &snapshot, &language) {
                            row_delta = Self::indent_selection(
                                buffer, &snapshot, selection, &mut edits, row_delta, cx,
                            );
                            continue;
                        }
                    }
                }
            }

            // If the selection is non-empty, then increase the indentation of the selected lines.
            if !selection.is_empty() {
                row_delta =
                    Self::indent_selection(buffer, &snapshot, selection, &mut edits, row_delta, cx);
                continue;
            }

            let cursor = selection.head();
            let current_indent = snapshot.indent_size_for_line(MultiBufferRow(cursor.row));
            if let Some(suggested_indent) =
                suggested_indents.get(&MultiBufferRow(cursor.row)).copied()
            {
                // Don't do anything if already at suggested indent
                // and there is any other cursor which is not
                if has_some_cursor_in_whitespace
                    && cursor.column == current_indent.len
                    && current_indent.len == suggested_indent.len
                {
                    continue;
                }

                // Adjust line and move cursor to suggested indent
                // if cursor is not at suggested indent
                if cursor.column < suggested_indent.len
                    && cursor.column <= current_indent.len
                    && current_indent.len <= suggested_indent.len
                {
                    selection.start = Point::new(cursor.row, suggested_indent.len);
                    selection.end = selection.start;
                    if row_delta == 0 {
                        edits.extend(Buffer::edit_for_indent_size_adjustment(
                            cursor.row,
                            current_indent,
                            suggested_indent,
                        ));
                        row_delta = suggested_indent.len - current_indent.len;
                    }
                    continue;
                }

                // If current indent is more than suggested indent
                // only move cursor to current indent and skip indent
                if cursor.column < current_indent.len && current_indent.len > suggested_indent.len {
                    selection.start = Point::new(cursor.row, current_indent.len);
                    selection.end = selection.start;
                    continue;
                }
            }

            // Otherwise, insert a hard or soft tab.
            let settings = buffer.language_settings_at(cursor, cx);
            let tab_size = if settings.hard_tabs {
                IndentSize::tab()
            } else {
                let tab_size = settings.tab_size.get();
                let indent_remainder = snapshot
                    .text_for_range(Point::new(cursor.row, 0)..cursor)
                    .flat_map(str::chars)
                    .fold(row_delta % tab_size, |counter: u32, c| {
                        if c == '\t' {
                            0
                        } else {
                            (counter + 1) % tab_size
                        }
                    });

                let chars_to_next_tab_stop = tab_size - indent_remainder;
                IndentSize::spaces(chars_to_next_tab_stop)
            };
            selection.start = Point::new(cursor.row, cursor.column + row_delta + tab_size.len);
            selection.end = selection.start;
            edits.push((cursor..cursor, tab_size.chars().collect::<String>()));
            row_delta += tab_size.len;
        }

        self.transact(window, cx, |this, window, cx| {
            this.buffer.update(cx, |buffer, cx| {
                buffer.edit_with_authorship(edits, None, AuthorshipSource::Human, cx)
            });
            this.change_selections(Default::default(), window, cx, |s| s.select(selections));
            this.refresh_edit_prediction(true, false, window, cx);
        });
    }

    pub fn indent(&mut self, _: &Indent, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        let mut selections = self.selections.all::<Point>(&self.display_snapshot(cx));
        let mut prev_edited_row = 0;
        let mut row_delta = 0;
        let mut edits = Vec::new();
        let buffer = self.buffer.read(cx);
        let snapshot = buffer.snapshot(cx);
        for selection in &mut selections {
            if selection.start.row != prev_edited_row {
                row_delta = 0;
            }
            prev_edited_row = selection.end.row;

            row_delta =
                Self::indent_selection(buffer, &snapshot, selection, &mut edits, row_delta, cx);
        }

        self.transact(window, cx, |this, window, cx| {
            this.buffer.update(cx, |b, cx| b.edit(edits, None, cx));
            this.change_selections(Default::default(), window, cx, |s| s.select(selections));
        });
    }

    fn indent_selection(
        buffer: &MultiBuffer,
        snapshot: &MultiBufferSnapshot,
        selection: &mut Selection<Point>,
        edits: &mut Vec<(Range<Point>, String)>,
        delta_for_start_row: u32,
        cx: &App,
    ) -> u32 {
        let settings = buffer.language_settings_at(selection.start, cx);
        let tab_size = settings.tab_size.get();
        let indent_kind = if settings.hard_tabs {
            IndentKind::Tab
        } else {
            IndentKind::Space
        };
        let mut start_row = selection.start.row;
        let mut end_row = selection.end.row + 1;

        // If a selection ends at the beginning of a line, don't indent
        // that last line.
        if selection.end.column == 0 && selection.end.row > selection.start.row {
            end_row -= 1;
        }

        // Avoid re-indenting a row that has already been indented by a
        // previous selection, but still update this selection's column
        // to reflect that indentation.
        if delta_for_start_row > 0 {
            start_row += 1;
            selection.start.column += delta_for_start_row;
            if selection.end.row == selection.start.row {
                selection.end.column += delta_for_start_row;
            }
        }

        let mut delta_for_end_row = 0;
        let has_multiple_rows = start_row + 1 != end_row;
        for row in start_row..end_row {
            let current_indent = snapshot.indent_size_for_line(MultiBufferRow(row));
            let indent_delta = match (current_indent.kind, indent_kind) {
                (IndentKind::Space, IndentKind::Space) => {
                    let columns_to_next_tab_stop = tab_size - (current_indent.len % tab_size);
                    IndentSize::spaces(columns_to_next_tab_stop)
                }
                (IndentKind::Tab, IndentKind::Space) => IndentSize::spaces(tab_size),
                (_, IndentKind::Tab) => IndentSize::tab(),
            };

            let start = if has_multiple_rows || current_indent.len < selection.start.column {
                0
            } else {
                selection.start.column
            };
            let row_start = Point::new(row, start);
            edits.push((
                row_start..row_start,
                indent_delta.chars().collect::<String>(),
            ));

            // Update this selection's endpoints to reflect the indentation.
            if row == selection.start.row {
                selection.start.column += indent_delta.len;
            }
            if row == selection.end.row {
                selection.end.column += indent_delta.len;
                delta_for_end_row = indent_delta.len;
            }
        }

        if selection.start.row == selection.end.row {
            delta_for_start_row + delta_for_end_row
        } else {
            delta_for_end_row
        }
    }

    pub fn outdent(&mut self, _: &Outdent, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let selections = self.selections.all::<Point>(&display_map);
        let mut deletion_ranges = Vec::new();
        let mut last_outdent = None;
        {
            let buffer = self.buffer.read(cx);
            let snapshot = buffer.snapshot(cx);
            for selection in &selections {
                let settings = buffer.language_settings_at(selection.start, cx);
                let tab_size = settings.tab_size.get();
                let mut rows = selection.spanned_rows(false, &display_map);

                // Avoid re-outdenting a row that has already been outdented by a
                // previous selection.
                if let Some(last_row) = last_outdent
                    && last_row == rows.start
                {
                    rows.start = rows.start.next_row();
                }
                let has_multiple_rows = rows.len() > 1;
                for row in rows.iter_rows() {
                    let indent_size = snapshot.indent_size_for_line(row);
                    if indent_size.len > 0 {
                        let deletion_len = match indent_size.kind {
                            IndentKind::Space => {
                                let columns_to_prev_tab_stop = indent_size.len % tab_size;
                                if columns_to_prev_tab_stop == 0 {
                                    tab_size
                                } else {
                                    columns_to_prev_tab_stop
                                }
                            }
                            IndentKind::Tab => 1,
                        };
                        let start = if has_multiple_rows
                            || deletion_len > selection.start.column
                            || indent_size.len < selection.start.column
                        {
                            0
                        } else {
                            selection.start.column - deletion_len
                        };
                        deletion_ranges.push(
                            Point::new(row.0, start)..Point::new(row.0, start + deletion_len),
                        );
                        last_outdent = Some(row);
                    }
                }
            }
        }

        self.transact(window, cx, |this, window, cx| {
            this.buffer.update(cx, |buffer, cx| {
                let empty_str: Arc<str> = Arc::default();
                buffer.edit_with_authorship(
                    deletion_ranges
                        .into_iter()
                        .map(|range| (range, empty_str.clone())),
                    None,
                    AuthorshipSource::Human,
                    cx,
                );
            });
            let selections = this
                .selections
                .all::<MultiBufferOffset>(&this.display_snapshot(cx));
            this.change_selections(Default::default(), window, cx, |s| s.select(selections));
        });
    }

    pub fn autoindent(&mut self, _: &AutoIndent, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        let selections = self
            .selections
            .all::<MultiBufferOffset>(&self.display_snapshot(cx))
            .into_iter()
            .map(|s| s.range());

        self.transact(window, cx, |this, window, cx| {
            this.buffer.update(cx, |buffer, cx| {
                buffer.autoindent_ranges(selections, cx);
            });
            let selections = this
                .selections
                .all::<MultiBufferOffset>(&this.display_snapshot(cx));
            this.change_selections(Default::default(), window, cx, |s| s.select(selections));
        });
    }

    pub fn delete_line(&mut self, _: &DeleteLine, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }
        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let selections = self.selections.all::<Point>(&display_map);

        let mut new_cursors = Vec::new();
        let mut edit_ranges = Vec::new();
        let mut selections = selections.iter().peekable();
        while let Some(selection) = selections.next() {
            let mut rows = selection.spanned_rows(false, &display_map);

            // Accumulate contiguous regions of rows that we want to delete.
            while let Some(next_selection) = selections.peek() {
                let next_rows = next_selection.spanned_rows(false, &display_map);
                if next_rows.start <= rows.end {
                    rows.end = next_rows.end;
                    selections.next().unwrap();
                } else {
                    break;
                }
            }

            let buffer = display_map.buffer_snapshot();
            let mut edit_start = ToOffset::to_offset(&Point::new(rows.start.0, 0), buffer);
            let (edit_end, target_row) = if buffer.max_point().row >= rows.end.0 {
                // If there's a line after the range, delete the \n from the end of the row range
                (
                    ToOffset::to_offset(&Point::new(rows.end.0, 0), buffer),
                    rows.end,
                )
            } else {
                // If there isn't a line after the range, delete the \n from the line before the
                // start of the row range
                edit_start = edit_start.saturating_sub_usize(1);
                (buffer.len(), rows.start.previous_row())
            };

            let text_layout_details = self.text_layout_details(window, cx);
            let x = display_map.x_for_display_point(
                selection.head().to_display_point(&display_map),
                &text_layout_details,
            );
            let row = Point::new(target_row.0, 0)
                .to_display_point(&display_map)
                .row();
            let column = display_map.display_column_for_x(row, x, &text_layout_details);

            new_cursors.push((
                selection.id,
                buffer.anchor_after(DisplayPoint::new(row, column).to_point(&display_map)),
                SelectionGoal::None,
            ));
            edit_ranges.push(edit_start..edit_end);
        }

        self.transact(window, cx, |this, window, cx| {
            let buffer = this.buffer.update(cx, |buffer, cx| {
                let empty_str: Arc<str> = Arc::default();
                buffer.edit_with_authorship(
                    edit_ranges
                        .into_iter()
                        .map(|range| (range, empty_str.clone())),
                    None,
                    AuthorshipSource::Human,
                    cx,
                );
                buffer.snapshot(cx)
            });
            let new_selections = new_cursors
                .into_iter()
                .map(|(id, cursor, goal)| {
                    let cursor = cursor.to_point(&buffer);
                    Selection {
                        id,
                        start: cursor,
                        end: cursor,
                        reversed: false,
                        goal,
                    }
                })
                .collect();

            this.change_selections(Default::default(), window, cx, |s| {
                s.select(new_selections);
            });
        });
    }

    pub fn join_lines_impl(
        &mut self,
        insert_whitespace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only(cx) {
            return;
        }
        let mut row_ranges = Vec::<Range<MultiBufferRow>>::new();
        for selection in self.selections.all::<Point>(&self.display_snapshot(cx)) {
            let start = MultiBufferRow(selection.start.row);
            // Treat single line selections as if they include the next line. Otherwise this action
            // would do nothing for single line selections individual cursors.
            let end = if selection.start.row == selection.end.row {
                MultiBufferRow(selection.start.row + 1)
            } else if selection.end.column == 0 {
                // If the selection ends at the start of a line, it's logically at the end of the
                // previous line (plus its newline).
                // Don't include the end line unless there's only one line selected.
                if selection.start.row + 1 == selection.end.row {
                    MultiBufferRow(selection.end.row)
                } else {
                    MultiBufferRow(selection.end.row - 1)
                }
            } else {
                MultiBufferRow(selection.end.row)
            };

            if let Some(last_row_range) = row_ranges.last_mut()
                && start <= last_row_range.end
            {
                last_row_range.end = end;
                continue;
            }
            row_ranges.push(start..end);
        }

        let snapshot = self.buffer.read(cx).snapshot(cx);
        let mut cursor_positions = Vec::new();
        for row_range in &row_ranges {
            let anchor = snapshot.anchor_before(Point::new(
                row_range.end.previous_row().0,
                snapshot.line_len(row_range.end.previous_row()),
            ));
            cursor_positions.push(anchor..anchor);
        }

        self.transact(window, cx, |this, window, cx| {
            for row_range in row_ranges.into_iter().rev() {
                for row in row_range.iter_rows().rev() {
                    let end_of_line = Point::new(row.0, snapshot.line_len(row));
                    let next_line_row = row.next_row();
                    let indent = snapshot.indent_size_for_line(next_line_row);
                    let mut join_start_column = indent.len;

                    if let Some(language_scope) =
                        snapshot.language_scope_at(Point::new(next_line_row.0, indent.len))
                    {
                        let line_end =
                            Point::new(next_line_row.0, snapshot.line_len(next_line_row));
                        let line_text_after_indent = snapshot
                            .text_for_range(Point::new(next_line_row.0, indent.len)..line_end)
                            .collect::<String>();

                        if !line_text_after_indent.is_empty() {
                            let block_prefix = language_scope
                                .block_comment()
                                .map(|c| c.prefix.as_ref())
                                .filter(|p| !p.is_empty());
                            let doc_prefix = language_scope
                                .documentation_comment()
                                .map(|c| c.prefix.as_ref())
                                .filter(|p| !p.is_empty());
                            let all_prefixes = language_scope
                                .line_comment_prefixes()
                                .iter()
                                .map(|p| p.as_ref())
                                .chain(block_prefix)
                                .chain(doc_prefix)
                                .chain(language_scope.unordered_list().iter().map(|p| p.as_ref()));

                            let mut longest_prefix_len = None;
                            for prefix in all_prefixes {
                                let trimmed = prefix.trim_end();
                                if line_text_after_indent.starts_with(trimmed) {
                                    let candidate_len =
                                        if line_text_after_indent.starts_with(prefix) {
                                            prefix.len()
                                        } else {
                                            trimmed.len()
                                        };
                                    if longest_prefix_len.map_or(true, |len| candidate_len > len) {
                                        longest_prefix_len = Some(candidate_len);
                                    }
                                }
                            }

                            if let Some(prefix_len) = longest_prefix_len {
                                join_start_column =
                                    join_start_column.saturating_add(prefix_len as u32);
                            }
                        }
                    }

                    let start_of_next_line = Point::new(next_line_row.0, join_start_column);

                    let replace = if snapshot.line_len(next_line_row) > join_start_column
                        && insert_whitespace
                    {
                        " "
                    } else {
                        ""
                    };

                    this.buffer.update(cx, |buffer, cx| {
                        buffer.edit_with_authorship(
                            [(end_of_line..start_of_next_line, replace)],
                            None,
                            AuthorshipSource::Human,
                            cx,
                        )
                    });
                }
            }

            this.change_selections(Default::default(), window, cx, |s| {
                s.select_anchor_ranges(cursor_positions)
            });
        });
    }

    pub fn join_lines(&mut self, _: &JoinLines, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        self.join_lines_impl(true, window, cx);
    }

    fn enable_wrap_selections_in_tag(&self, cx: &App) -> bool {
        let snapshot = self.buffer.read(cx).snapshot(cx);
        for selection in self.selections.disjoint_anchors_arc().iter() {
            if snapshot
                .language_at(selection.start)
                .and_then(|lang| lang.config().wrap_characters.as_ref())
                .is_some()
            {
                return true;
            }
        }
        false
    }

    fn wrap_selections_in_tag(
        &mut self,
        _: &WrapSelectionsInTag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only(cx) {
            return;
        }
        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);

        let snapshot = self.buffer.read(cx).snapshot(cx);

        let mut edits = Vec::new();
        let mut boundaries = Vec::new();

        for selection in self
            .selections
            .all_adjusted(&self.display_snapshot(cx))
            .iter()
        {
            let Some(wrap_config) = snapshot
                .language_at(selection.start)
                .and_then(|lang| lang.config().wrap_characters.clone())
            else {
                continue;
            };

            let open_tag = format!("{}{}", wrap_config.start_prefix, wrap_config.start_suffix);
            let close_tag = format!("{}{}", wrap_config.end_prefix, wrap_config.end_suffix);

            let start_before = snapshot.anchor_before(selection.start);
            let end_after = snapshot.anchor_after(selection.end);

            edits.push((start_before..start_before, open_tag));
            edits.push((end_after..end_after, close_tag));

            boundaries.push((
                start_before,
                end_after,
                wrap_config.start_prefix.len(),
                wrap_config.end_suffix.len(),
            ));
        }

        if edits.is_empty() {
            return;
        }

        self.transact(window, cx, |this, window, cx| {
            let buffer = this.buffer.update(cx, |buffer, cx| {
                buffer.edit_with_authorship(edits, None, AuthorshipSource::Human, cx);
                buffer.snapshot(cx)
            });

            let mut new_selections = Vec::with_capacity(boundaries.len() * 2);
            for (start_before, end_after, start_prefix_len, end_suffix_len) in
                boundaries.into_iter()
            {
                let open_offset = start_before.to_offset(&buffer) + start_prefix_len;
                let close_offset = end_after
                    .to_offset(&buffer)
                    .saturating_sub_usize(end_suffix_len);
                new_selections.push(open_offset..open_offset);
                new_selections.push(close_offset..close_offset);
            }

            this.change_selections(Default::default(), window, cx, |s| {
                s.select_ranges(new_selections);
            });

            this.request_autoscroll(Autoscroll::fit(), cx);
        });
    }

    pub fn toggle_read_only(
        &mut self,
        _: &workspace::ToggleReadOnlyFile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(buffer) = self.buffer.read(cx).as_singleton() {
            buffer.update(cx, |buffer, cx| {
                buffer.set_capability(
                    match buffer.capability() {
                        Capability::ReadWrite => Capability::Read,
                        Capability::Read => Capability::ReadWrite,
                        Capability::ReadOnly => Capability::ReadOnly,
                    },
                    cx,
                );
            })
        }
    }

    pub fn reload_file(&mut self, _: &ReloadFile, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        let task = self.reload(project, window, cx);
        self.detach_and_notify_err(task, window, cx);
    }

    pub fn highlighted_display_row_for_autoscroll(
        &self,
        snapshot: &DisplaySnapshot,
    ) -> Option<DisplayRow> {
        self.highlighted_rows
            .values()
            .flat_map(|highlighted_rows| highlighted_rows.iter())
            .filter_map(|highlight| {
                if highlight.options.autoscroll {
                    Some(highlight.range.start.to_display_point(snapshot).row())
                } else {
                    None
                }
            })
            .min()
    }

    pub fn highlight_background(
        &mut self,
        key: HighlightKey,
        ranges: &[Range<Anchor>],
        color_fetcher: impl Fn(&usize, &Theme) -> Hsla + Send + Sync + 'static,
        cx: &mut Context<Self>,
    ) {
        self.background_highlights
            .insert(key, (Arc::new(color_fetcher), Arc::from(ranges)));
        self.scrollbar_marker_state.dirty = true;
        cx.notify();
    }

    pub fn clear_background_highlights(
        &mut self,
        key: HighlightKey,
        cx: &mut Context<Self>,
    ) -> Option<BackgroundHighlight> {
        let text_highlights = self.background_highlights.remove(&key)?;
        if !text_highlights.1.is_empty() {
            self.scrollbar_marker_state.dirty = true;
            cx.notify();
        }
        Some(text_highlights)
    }

    pub fn highlight_gutter<T: 'static>(
        &mut self,
        ranges: impl Into<Vec<Range<Anchor>>>,
        color_fetcher: fn(&App) -> Hsla,
        cx: &mut Context<Self>,
    ) {
        self.gutter_highlights
            .insert(TypeId::of::<T>(), (color_fetcher, ranges.into()));
        cx.notify();
    }

    pub fn clear_gutter_highlights<T: 'static>(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<GutterHighlight> {
        cx.notify();
        self.gutter_highlights.remove(&TypeId::of::<T>())
    }

    pub fn insert_gutter_highlight<T: 'static>(
        &mut self,
        range: Range<Anchor>,
        color_fetcher: fn(&App) -> Hsla,
        cx: &mut Context<Self>,
    ) {
        let snapshot = self.buffer().read(cx).snapshot(cx);
        let mut highlights = self
            .gutter_highlights
            .remove(&TypeId::of::<T>())
            .map(|(_, highlights)| highlights)
            .unwrap_or_default();
        let ix = highlights.binary_search_by(|highlight| {
            Ordering::Equal
                .then_with(|| highlight.start.cmp(&range.start, &snapshot))
                .then_with(|| highlight.end.cmp(&range.end, &snapshot))
        });
        if let Err(ix) = ix {
            highlights.insert(ix, range);
        }
        self.gutter_highlights
            .insert(TypeId::of::<T>(), (color_fetcher, highlights));
    }

    pub fn remove_gutter_highlights<T: 'static>(
        &mut self,
        ranges_to_remove: Vec<Range<Anchor>>,
        cx: &mut Context<Self>,
    ) {
        let snapshot = self.buffer().read(cx).snapshot(cx);
        let Some((color_fetcher, mut gutter_highlights)) =
            self.gutter_highlights.remove(&TypeId::of::<T>())
        else {
            return;
        };
        let mut ranges_to_remove = ranges_to_remove.iter().peekable();
        gutter_highlights.retain(|highlight| {
            while let Some(range_to_remove) = ranges_to_remove.peek() {
                match range_to_remove.end.cmp(&highlight.start, &snapshot) {
                    Ordering::Less | Ordering::Equal => {
                        ranges_to_remove.next();
                    }
                    Ordering::Greater => {
                        match range_to_remove.start.cmp(&highlight.end, &snapshot) {
                            Ordering::Less | Ordering::Equal => {
                                return false;
                            }
                            Ordering::Greater => break,
                        }
                    }
                }
            }

            true
        });
        self.gutter_highlights
            .insert(TypeId::of::<T>(), (color_fetcher, gutter_highlights));
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn all_text_highlights(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<(HighlightStyle, Vec<Range<DisplayPoint>>)> {
        let snapshot = self.snapshot(window, cx);
        self.display_map.update(cx, |display_map, _| {
            display_map
                .all_text_highlights()
                .map(|(_, highlight)| {
                    let (style, ranges) = highlight.as_ref();
                    (
                        *style,
                        ranges
                            .iter()
                            .map(|range| range.clone().to_display_points(&snapshot))
                            .collect(),
                    )
                })
                .collect()
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn all_text_background_highlights(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<(Range<DisplayPoint>, Hsla)> {
        let snapshot = self.snapshot(window, cx);
        let buffer = &snapshot.buffer_snapshot();
        let start = buffer.anchor_before(MultiBufferOffset(0));
        let end = buffer.anchor_after(buffer.len());
        self.sorted_background_highlights_in_range(start..end, &snapshot, cx.theme())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn sorted_background_highlights_in_range(
        &self,
        search_range: Range<Anchor>,
        display_snapshot: &DisplaySnapshot,
        theme: &Theme,
    ) -> Vec<(Range<DisplayPoint>, Hsla)> {
        let mut res = self.background_highlights_in_range(search_range, display_snapshot, theme);
        res.sort_by(|a, b| {
            a.0.start
                .cmp(&b.0.start)
                .then_with(|| a.0.end.cmp(&b.0.end))
                .then_with(|| a.1.cmp(&b.1))
        });
        res
    }

    pub fn has_background_highlights(&self, key: HighlightKey) -> bool {
        self.background_highlights
            .get(&key)
            .is_some_and(|(_, highlights)| !highlights.is_empty())
    }

    /// Returns all background highlights for a given range.
    ///
    /// The order of highlights is not deterministic, do sort the ranges if needed for the logic.
    pub fn background_highlights_in_range(
        &self,
        search_range: Range<Anchor>,
        display_snapshot: &DisplaySnapshot,
        theme: &Theme,
    ) -> Vec<(Range<DisplayPoint>, Hsla)> {
        let mut results = Vec::new();
        for (color_fetcher, ranges) in self.background_highlights.values() {
            let start_ix = match ranges.binary_search_by(|probe| {
                let cmp = probe
                    .end
                    .cmp(&search_range.start, &display_snapshot.buffer_snapshot());
                if cmp.is_gt() {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }) {
                Ok(i) | Err(i) => i,
            };
            for (index, range) in ranges[start_ix..].iter().enumerate() {
                if range
                    .start
                    .cmp(&search_range.end, &display_snapshot.buffer_snapshot())
                    .is_ge()
                {
                    break;
                }

                let color = color_fetcher(&(start_ix + index), theme);
                let start = range.start.to_display_point(display_snapshot);
                let end = range.end.to_display_point(display_snapshot);
                results.push((start..end, color))
            }
        }
        results
    }

    pub fn gutter_highlights_in_range(
        &self,
        search_range: Range<Anchor>,
        display_snapshot: &DisplaySnapshot,
        cx: &App,
    ) -> Vec<(Range<DisplayPoint>, Hsla)> {
        let mut results = Vec::new();
        for (color_fetcher, ranges) in self.gutter_highlights.values() {
            let color = color_fetcher(cx);
            let start_ix = match ranges.binary_search_by(|probe| {
                let cmp = probe
                    .end
                    .cmp(&search_range.start, &display_snapshot.buffer_snapshot());
                if cmp.is_gt() {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }) {
                Ok(i) | Err(i) => i,
            };
            for range in &ranges[start_ix..] {
                if range
                    .start
                    .cmp(&search_range.end, &display_snapshot.buffer_snapshot())
                    .is_ge()
                {
                    break;
                }

                let start = range.start.to_display_point(display_snapshot);
                let end = range.end.to_display_point(display_snapshot);
                results.push((start..end, color))
            }
        }
        results
    }

    /// Get the text ranges corresponding to the redaction query
    pub fn redacted_ranges(
        &self,
        search_range: Range<Anchor>,
        display_snapshot: &DisplaySnapshot,
        cx: &App,
    ) -> Vec<Range<DisplayPoint>> {
        display_snapshot
            .buffer_snapshot()
            .redacted_ranges(search_range, |file| {
                if let Some(file) = file {
                    file.is_private()
                        && EditorSettings::get(
                            Some(SettingsLocation {
                                worktree_id: file.worktree_id(cx),
                                path: file.path().as_ref(),
                            }),
                            cx,
                        )
                        .redact_private_values
                } else {
                    false
                }
            })
            .map(|range| {
                range.start.to_display_point(display_snapshot)
                    ..range.end.to_display_point(display_snapshot)
            })
            .collect()
    }

    pub fn highlight_text_key(
        &mut self,
        key: HighlightKey,
        ranges: Vec<Range<Anchor>>,
        style: HighlightStyle,
        merge: bool,
        cx: &mut Context<Self>,
    ) {
        self.display_map.update(cx, |map, cx| {
            map.highlight_text(key, ranges, style, merge, cx);
        });
        cx.notify();
    }

    pub fn highlight_text(
        &mut self,
        key: HighlightKey,
        ranges: Vec<Range<Anchor>>,
        style: HighlightStyle,
        cx: &mut Context<Self>,
    ) {
        self.display_map.update(cx, |map, cx| {
            map.highlight_text(key, ranges, style, false, cx)
        });
        cx.notify();
    }

    pub fn text_highlights<'a>(
        &'a self,
        key: HighlightKey,
        cx: &'a App,
    ) -> Option<(HighlightStyle, &'a [Range<Anchor>])> {
        self.display_map.read(cx).text_highlights(key)
    }

    pub fn set_navigation_overlays(
        &mut self,
        key: NavigationOverlayKey,
        overlays: Vec<NavigationTargetOverlay>,
        cx: &mut Context<Self>,
    ) {
        let buffer_snapshot = self.buffer.read(cx).snapshot(cx);
        let mut covered_text_ranges = overlays
            .iter()
            .filter_map(|overlay| overlay.covered_text_range.clone())
            .collect::<Vec<_>>();
        covered_text_ranges.sort_by(|left, right| {
            left.start
                .cmp(&right.start, &buffer_snapshot)
                .then_with(|| left.end.cmp(&right.end, &buffer_snapshot))
        });

        self.display_map.update(cx, |map, cx| {
            map.clear_highlights(HighlightKey::NavigationOverlay(key));
            if !covered_text_ranges.is_empty() {
                map.highlight_text(
                    HighlightKey::NavigationOverlay(key),
                    covered_text_ranges,
                    HighlightStyle {
                        fade_out: Some(1.0),
                        ..Default::default()
                    },
                    false,
                    cx,
                );
            }
        });

        if overlays.is_empty() {
            self.navigation_overlays.remove(&key);
        } else {
            self.navigation_overlays.insert(key, Arc::from(overlays));
        }

        cx.notify();
    }

    pub fn clear_navigation_overlays(&mut self, key: NavigationOverlayKey, cx: &mut Context<Self>) {
        let removed = self.navigation_overlays.remove(&key).is_some();
        let cleared = self.display_map.update(cx, |map, _| {
            map.clear_highlights(HighlightKey::NavigationOverlay(key))
        });
        if removed || cleared {
            cx.notify();
        }
    }

    pub(crate) fn navigation_overlay_sets(
        &self,
    ) -> &HashMap<NavigationOverlayKey, Arc<[NavigationTargetOverlay]>> {
        &self.navigation_overlays
    }

    pub fn clear_highlights(&mut self, key: HighlightKey, cx: &mut Context<Self>) {
        let cleared = self
            .display_map
            .update(cx, |map, _| map.clear_highlights(key));
        if cleared {
            cx.notify();
        }
    }

    pub fn clear_highlights_with(
        &mut self,
        f: &mut dyn FnMut(&HighlightKey) -> bool,
        cx: &mut Context<Self>,
    ) {
        let cleared = self
            .display_map
            .update(cx, |map, _| map.clear_highlights_with(f));
        if cleared {
            cx.notify();
        }
    }

    pub fn show_local_cursors(&self, window: &mut Window, cx: &mut App) -> bool {
        (self.read_only(cx) || self.blink_manager.read(cx).visible())
            && self.focus_handle.is_focused(window)
    }

    pub fn set_show_cursor_when_unfocused(&mut self, is_enabled: bool, cx: &mut Context<Self>) {
        self.show_cursor_when_unfocused = is_enabled;
        cx.notify();
    }

    fn on_buffer_changed(&mut self, _: Entity<MultiBuffer>, cx: &mut Context<Self>) {
        cx.notify();
    }

    fn on_buffer_event(
        &mut self,
        _multibuffer: &Entity<MultiBuffer>,
        event: &multi_buffer::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            multi_buffer::Event::Edited {
                edited_buffer,
                is_local,
            } => {
                self.scrollbar_marker_state.dirty = true;
                self.active_indent_guides_state.dirty = true;
                self.refresh_single_line_folds(window, cx);
                let snapshot = self.snapshot(window, cx);
                self.refresh_matching_bracket_highlights(&snapshot, cx);
                self.refresh_sticky_headers(&snapshot, cx);
                if *is_local && self.has_active_edit_prediction() {
                    self.update_visible_edit_prediction(window, cx);
                }
                self.refresh_authorship_highlights(cx);
                self.persist_authorship(cx);

                if let Some(buffer) = edited_buffer {
                    if buffer.read(cx).file().is_none() {
                        cx.emit(EditorEvent::TitleChanged);
                    }
                }

                cx.emit(EditorEvent::BufferEdited);
            }
            multi_buffer::Event::BufferRangesUpdated {
                buffer,
                ranges,
                path_key,
            } => {
                let buffer_id = buffer.read(cx).remote_id();
                self.bracket_fetched_tree_sitter_chunks
                    .retain(|range, _| range.start.buffer_id != buffer_id);
                self.colorize_brackets(false, cx);
                self.refresh_selected_text_highlights(&self.display_snapshot(cx), true, window, cx);
                cx.emit(EditorEvent::BufferRangesUpdated {
                    buffer: buffer.clone(),
                    ranges: ranges.clone(),
                    path_key: path_key.clone(),
                });
            }
            multi_buffer::Event::BuffersRemoved { removed_buffer_ids } => {
                self.display_map.update(cx, |display_map, cx| {
                    display_map.unfold_buffers(removed_buffer_ids.iter().copied(), cx);
                });

                cx.emit(EditorEvent::BuffersRemoved {
                    removed_buffer_ids: removed_buffer_ids.clone(),
                });
            }
            multi_buffer::Event::BuffersEdited { buffer_ids } => {
                self.display_map.update(cx, |map, cx| {
                    map.unfold_buffers(buffer_ids.iter().copied(), cx)
                });
                cx.emit(EditorEvent::BuffersEdited {
                    buffer_ids: buffer_ids.clone(),
                });
            }
            multi_buffer::Event::Reparsed(buffer_id) => {
                self.refresh_selected_text_highlights(&self.display_snapshot(cx), true, window, cx);
                self.colorize_brackets(true, cx);

                cx.emit(EditorEvent::Reparsed(*buffer_id));
            }
            multi_buffer::Event::LanguageChanged(buffer_id, _) => {
                cx.emit(EditorEvent::Reparsed(*buffer_id));
                cx.notify();
            }
            multi_buffer::Event::DirtyChanged => cx.emit(EditorEvent::DirtyChanged),
            multi_buffer::Event::Saved => {
                self.persist_authorship_now(cx);
                cx.emit(EditorEvent::Saved)
            }
            multi_buffer::Event::FileHandleChanged => {
                cx.emit(EditorEvent::TitleChanged);
                cx.emit(EditorEvent::FileHandleChanged);
            }
            multi_buffer::Event::Reloaded => {
                self.refresh_authorship_highlights(cx);
                self.persist_authorship(cx);
                cx.emit(EditorEvent::TitleChanged)
            }
            _ => {}
        };
    }

    pub fn start_temporary_diff_override(&mut self) {}

    pub fn end_temporary_diff_override(&mut self, _cx: &mut Context<Self>) {}

    fn on_display_map_changed(
        &mut self,
        _: Entity<DisplayMap>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
    }

    fn fetch_accent_data(&self, cx: &App) -> Option<AccentData> {
        if !self.mode.is_full() {
            return None;
        }

        let theme_settings = theme_settings::ThemeSettings::get_global(cx);
        let theme = cx.theme();
        let accent_colors = theme.accents().clone();

        let accent_overrides = theme_settings
            .theme_overrides
            .get(theme.name.as_ref())
            .map(|theme_style| &theme_style.accents)
            .into_iter()
            .flatten()
            .chain(
                theme_settings
                    .experimental_theme_overrides
                    .as_ref()
                    .map(|overrides| &overrides.accents)
                    .into_iter()
                    .flatten(),
            )
            .flat_map(|accent| accent.0.clone().map(SharedString::from))
            .collect();

        Some(AccentData {
            colors: accent_colors,
            overrides: accent_overrides,
        })
    }

    fn fetch_applicable_language_settings(
        &self,
        cx: &App,
    ) -> HashMap<Option<LanguageName>, LanguageSettings> {
        if !self.mode.is_full() {
            return HashMap::default();
        }

        self.buffer().read(cx).all_buffers().into_iter().fold(
            HashMap::default(),
            |mut acc, buffer| {
                let buffer = buffer.read(cx);
                let language = buffer.language().map(|language| language.name());
                if let hash_map::Entry::Vacant(v) = acc.entry(language) {
                    v.insert(LanguageSettings::for_buffer(&buffer, cx).into_owned());
                }
                acc
            },
        )
    }

    fn settings_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let new_language_settings = self.fetch_applicable_language_settings(cx);
        let language_settings_changed = new_language_settings != self.applicable_language_settings;
        self.applicable_language_settings = new_language_settings;

        let new_accents = self.fetch_accent_data(cx);
        let accents_changed = new_accents != self.accent_data;
        self.accent_data = new_accents;

        self.refresh_edit_prediction(true, false, window, cx);

        let old_cursor_shape = self.cursor_shape;

        {
            let editor_settings = EditorSettings::get_global(cx);
            self.scroll_manager.vertical_scroll_margin = editor_settings.vertical_scroll_margin;
            self.cursor_shape = editor_settings.cursor_shape.unwrap_or_default();
            self.hide_mouse_mode = editor_settings.hide_mouse.unwrap_or_default();
        }

        if old_cursor_shape != self.cursor_shape {
            cx.emit(EditorEvent::CursorShapeChanged);
        }

        if self.mode.is_full() {
            if language_settings_changed || accents_changed {
                self.colorize_brackets(true, cx);
            }
        }

        cx.notify();
    }

    fn theme_changed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if !self.mode.is_full() {
            return;
        }

        let new_accents = self.fetch_accent_data(cx);
        if new_accents != self.accent_data {
            self.accent_data = new_accents;
            self.colorize_brackets(true, cx);
        }
    }

    pub fn open_excerpts_in_split(
        &mut self,
        _: &OpenExcerptsSplit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_excerpts_common(None, true, window, cx)
    }

    pub fn open_excerpts(&mut self, _: &OpenExcerpts, window: &mut Window, cx: &mut Context<Self>) {
        self.open_excerpts_common(None, false, window, cx)
    }

    pub(crate) fn open_excerpts_common(
        &mut self,
        jump_data: Option<JumpData>,
        split: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.buffer.read(cx).is_singleton() {
            cx.propagate();
            return;
        }

        let mut new_selections_by_buffer = HashMap::default();
        match &jump_data {
            Some(JumpData::MultiBufferPoint {
                anchor,
                position,
                line_offset_from_top,
            }) => {
                if let Some(buffer) = self.buffer.read(cx).buffer(anchor.buffer_id) {
                    let buffer_snapshot = buffer.read(cx).snapshot();
                    let jump_to_point = if buffer_snapshot.can_resolve(&anchor) {
                        language::ToPoint::to_point(anchor, &buffer_snapshot)
                    } else {
                        buffer_snapshot.clip_point(*position, Bias::Left)
                    };
                    let jump_to_offset = buffer_snapshot.point_to_offset(jump_to_point);
                    new_selections_by_buffer.insert(
                        buffer,
                        (
                            vec![BufferOffset(jump_to_offset)..BufferOffset(jump_to_offset)],
                            Some(*line_offset_from_top),
                        ),
                    );
                }
            }
            Some(JumpData::MultiBufferRow {
                row,
                line_offset_from_top,
            }) => {
                let point = MultiBufferPoint::new(row.0, 0);
                if let Some((buffer, buffer_point)) =
                    self.buffer.read(cx).point_to_buffer_point(point, cx)
                {
                    let buffer_offset = buffer.read(cx).point_to_offset(buffer_point);
                    new_selections_by_buffer
                        .entry(buffer)
                        .or_insert((Vec::new(), Some(*line_offset_from_top)))
                        .0
                        .push(BufferOffset(buffer_offset)..BufferOffset(buffer_offset))
                }
            }
            None => {
                let selections = self
                    .selections
                    .all::<MultiBufferOffset>(&self.display_snapshot(cx));
                let multi_buffer = self.buffer.read(cx);
                let multi_buffer_snapshot = multi_buffer.snapshot(cx);
                for selection in selections {
                    for (snapshot, range, _) in
                        multi_buffer_snapshot.range_to_buffer_ranges(selection.range())
                    {
                        let Some(buffer_handle) = multi_buffer.buffer(snapshot.remote_id()) else {
                            continue;
                        };
                        new_selections_by_buffer
                            .entry(buffer_handle)
                            .or_insert((Vec::new(), None))
                            .0
                            .push(range)
                    }
                }
            }
        }

        if self.delegate_open_excerpts {
            let selections_by_buffer: HashMap<_, _> = new_selections_by_buffer
                .into_iter()
                .map(|(buffer, value)| (buffer.read(cx).remote_id(), value))
                .collect();
            if !selections_by_buffer.is_empty() {
                cx.emit(EditorEvent::OpenExcerptsRequested {
                    selections_by_buffer,
                    split,
                });
            }
            return;
        }

        let Some(workspace) = self.workspace() else {
            cx.propagate();
            return;
        };

        new_selections_by_buffer
            .retain(|buffer, _| buffer.read(cx).file().is_none_or(|file| file.can_open()));

        if new_selections_by_buffer.is_empty() {
            return;
        }

        Self::open_buffers_in_workspace(
            workspace.downgrade(),
            new_selections_by_buffer,
            split,
            window,
            cx,
        );
    }

    pub(crate) fn open_buffers_in_workspace(
        workspace: WeakEntity<Workspace>,
        new_selections_by_buffer: HashMap<
            Entity<language::Buffer>,
            (Vec<Range<BufferOffset>>, Option<u32>),
        >,
        split: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        // We defer the pane interaction because we ourselves are a workspace item
        // and activating a new item causes the pane to call a method on us reentrantly,
        // which panics if we're on the stack.
        window.defer(cx, move |window, cx| {
            workspace
                .update(cx, |workspace, cx| {
                    let pane = if split {
                        workspace.adjacent_pane(window, cx)
                    } else {
                        workspace.active_pane().clone()
                    };

                    for (buffer, (ranges, scroll_offset)) in new_selections_by_buffer {
                        let buffer_read = buffer.read(cx);
                        let (has_file, is_project_file) = if let Some(file) = buffer_read.file() {
                            (true, project::File::from_dyn(Some(file)).is_some())
                        } else {
                            (false, false)
                        };

                        // If project file is none workspace.open_project_item will fail to open the excerpt
                        // in a pre existing workspace item if one exists, because Buffer entity_id will be None
                        // so we check if there's a tab match in that case first
                        let editor = (!has_file || !is_project_file)
                            .then(|| {
                                // Handle file-less buffers separately: those are not really the project items, so won't have a project path or entity id,
                                // so `workspace.open_project_item` will never find them, always opening a new editor.
                                // Instead, we try to activate the existing editor in the pane first.
                                let (editor, pane_item_index, pane_item_id) =
                                    pane.read(cx).items().enumerate().find_map(|(i, item)| {
                                        let editor = item.downcast::<Editor>()?;
                                        let singleton_buffer =
                                            editor.read(cx).buffer().read(cx).as_singleton()?;
                                        if singleton_buffer == buffer {
                                            Some((editor, i, item.item_id()))
                                        } else {
                                            None
                                        }
                                    })?;
                                pane.update(cx, |pane, cx| {
                                    pane.activate_item(pane_item_index, true, true, window, cx);
                                    pane.unpreview_item_if_preview(pane_item_id);
                                });
                                Some(editor)
                            })
                            .flatten()
                            .unwrap_or_else(|| {
                                workspace.open_project_item::<Self>(
                                    pane.clone(),
                                    buffer,
                                    true,
                                    true,
                                    false,
                                    false,
                                    window,
                                    cx,
                                )
                            });

                        editor.update(cx, |editor, cx| {
                            if has_file && !is_project_file {
                                editor.set_read_only(true);
                            }
                            let autoscroll = match scroll_offset {
                                Some(scroll_offset) => {
                                    Autoscroll::top_relative(scroll_offset as ScrollOffset)
                                }
                                None => Autoscroll::newest(),
                            };
                            let nav_history = editor.nav_history.take();
                            let multibuffer_snapshot = editor.buffer().read(cx).snapshot(cx);
                            let Some(buffer_snapshot) = multibuffer_snapshot.as_singleton() else {
                                return;
                            };
                            editor.change_selections(
                                SelectionEffects::scroll(autoscroll),
                                window,
                                cx,
                                |s| {
                                    s.select_ranges(ranges.into_iter().map(|range| {
                                        let range = buffer_snapshot.anchor_before(range.start)
                                            ..buffer_snapshot.anchor_after(range.end);
                                        multibuffer_snapshot
                                            .buffer_anchor_range_to_anchor_range(range)
                                            .unwrap()
                                    }));
                                },
                            );
                            editor.nav_history = nav_history;
                        });
                    }
                })
                .ok();
        });
    }

    fn marked_text_ranges(&self, cx: &App) -> Option<Vec<Range<MultiBufferOffsetUtf16>>> {
        let snapshot = self.buffer.read(cx).read(cx);
        let (_, ranges) = self.text_highlights(HighlightKey::InputComposition, cx)?;
        Some(
            ranges
                .iter()
                .map(move |range| {
                    range.start.to_offset_utf16(&snapshot)..range.end.to_offset_utf16(&snapshot)
                })
                .collect(),
        )
    }

    fn selection_replacement_ranges(
        &self,
        range: Range<MultiBufferOffsetUtf16>,
        cx: &mut App,
    ) -> Vec<Range<MultiBufferOffsetUtf16>> {
        let selections = self
            .selections
            .all::<MultiBufferOffsetUtf16>(&self.display_snapshot(cx));
        let newest_selection = selections
            .iter()
            .max_by_key(|selection| selection.id)
            .unwrap();
        let start_delta = range.start.0.0 as isize - newest_selection.start.0.0 as isize;
        let end_delta = range.end.0.0 as isize - newest_selection.end.0.0 as isize;
        let snapshot = self.buffer.read(cx).read(cx);
        selections
            .into_iter()
            .map(|mut selection| {
                selection.start.0.0 =
                    (selection.start.0.0 as isize).saturating_add(start_delta) as usize;
                selection.end.0.0 = (selection.end.0.0 as isize).saturating_add(end_delta) as usize;
                snapshot.clip_offset_utf16(selection.start, Bias::Left)
                    ..snapshot.clip_offset_utf16(selection.end, Bias::Right)
            })
            .collect()
    }

    fn report_editor_event(
        &self,
        reported_event: ReportEditorEvent,
        file_extension: Option<String>,
        cx: &App,
    ) {
        let _ = (reported_event, file_extension, cx);
    }

    /// Copy the highlighted chunks to the clipboard as JSON. The format is an array of lines,
    /// with each line being an array of {text, highlight} objects.
    fn copy_highlight_json(
        &mut self,
        _: &CopyHighlightJson,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[derive(Serialize)]
        struct Chunk<'a> {
            text: String,
            highlight: Option<&'a str>,
        }

        let snapshot = self.buffer.read(cx).snapshot(cx);
        let mut selection = self.selections.newest::<Point>(&self.display_snapshot(cx));
        let max_point = snapshot.max_point();

        let range = if self.selections.line_mode() {
            selection.start = Point::new(selection.start.row, 0);
            selection.end = cmp::min(max_point, Point::new(selection.end.row + 1, 0));
            selection.goal = SelectionGoal::None;
            selection.range()
        } else if selection.is_empty() {
            Point::new(0, 0)..max_point
        } else {
            selection.range()
        };

        let chunks = snapshot.chunks(range, LanguageAwareStyling { tree_sitter: true });
        let mut lines = Vec::new();
        let mut line: VecDeque<Chunk> = VecDeque::new();

        let Some(style) = self.style.as_ref() else {
            return;
        };

        for chunk in chunks {
            let highlight = chunk
                .syntax_highlight_id
                .and_then(|id| style.syntax.get_capture_name(id));

            let mut chunk_lines = chunk.text.split('\n').peekable();
            while let Some(text) = chunk_lines.next() {
                let mut merged_with_last_token = false;
                if let Some(last_token) = line.back_mut()
                    && last_token.highlight == highlight
                {
                    last_token.text.push_str(text);
                    merged_with_last_token = true;
                }

                if !merged_with_last_token {
                    line.push_back(Chunk {
                        text: text.into(),
                        highlight,
                    });
                }

                if chunk_lines.peek().is_some() {
                    if line.len() > 1 && line.front().unwrap().text.is_empty() {
                        line.pop_front();
                    }
                    if line.len() > 1 && line.back().unwrap().text.is_empty() {
                        line.pop_back();
                    }

                    lines.push(mem::take(&mut line));
                }
            }
        }

        if line.iter().any(|chunk| !chunk.text.is_empty()) {
            lines.push(line);
        }

        let Some(lines) = serde_json::to_string_pretty(&lines).log_err() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(lines));
    }

    pub fn open_context_menu(
        &mut self,
        _: &OpenContextMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_autoscroll(Autoscroll::newest(), cx);
        let position = self
            .selections
            .newest_display(&self.display_snapshot(cx))
            .start;
        mouse_context_menu::deploy_context_menu(self, None, position, window, cx);
    }

    pub fn replay_insert_event(
        &mut self,
        text: &str,
        relative_utf16_range: Option<Range<isize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.input_enabled {
            cx.emit(EditorEvent::InputIgnored { text: text.into() });
            return;
        }
        if let Some(relative_utf16_range) = relative_utf16_range {
            let selections = self
                .selections
                .all::<MultiBufferOffsetUtf16>(&self.display_snapshot(cx));
            self.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                let new_ranges = selections.into_iter().map(|range| {
                    let start = MultiBufferOffsetUtf16(OffsetUtf16(
                        range
                            .head()
                            .0
                            .0
                            .saturating_add_signed(relative_utf16_range.start),
                    ));
                    let end = MultiBufferOffsetUtf16(OffsetUtf16(
                        range
                            .head()
                            .0
                            .0
                            .saturating_add_signed(relative_utf16_range.end),
                    ));
                    start..end
                });
                s.select_ranges(new_ranges);
            });
        }

        self.handle_input(text, window, cx);
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    fn handle_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(EditorEvent::Focused);

        if let Some(descendant) = self
            .last_focused_descendant
            .take()
            .and_then(|descendant| descendant.upgrade())
        {
            window.focus(&descendant, cx);
        } else {
            self.blink_manager.update(cx, BlinkManager::enable);
            self.buffer.update(cx, |buffer, cx| {
                buffer.finalize_last_transaction(cx);
                buffer.set_active_selections(
                    &self.selections.disjoint_anchors_arc(),
                    self.selections.line_mode(),
                    self.cursor_shape,
                    cx,
                );
            });

            if let Some(position_map) = self.last_position_map.clone()
                && !self.mouse_cursor_hidden
            {
                EditorElement::mouse_moved(
                    self,
                    &MouseMoveEvent {
                        position: window.mouse_position(),
                        pressed_button: None,
                        modifiers: window.modifiers(),
                    },
                    &position_map,
                    None,
                    window,
                    cx,
                );
            }
        }
    }

    fn handle_focus_in(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(EditorEvent::FocusedIn)
    }

    fn handle_focus_out(
        &mut self,
        event: FocusOutEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if event.blurred != self.focus_handle {
            self.last_focused_descendant = Some(event.blurred);
        }
        self.selection_drag_state = SelectionDragState::None;
    }

    pub fn handle_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.blink_manager.update(cx, BlinkManager::disable);
        self.buffer
            .update(cx, |buffer, cx| buffer.remove_active_selections(cx));

        if !self.hover_state.focused(window, cx) {
            hide_hover(self, cx);
        }
        cx.emit(EditorEvent::Blurred);
        cx.notify();
    }

    pub fn observe_pending_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut pending: String = window
            .pending_input_keystrokes()
            .into_iter()
            .flatten()
            .filter_map(|keystroke| keystroke.key_char.clone())
            .collect();

        if !self.input_enabled || self.read_only || !self.focus_handle.is_focused(window) {
            pending = "".to_string();
        }

        let existing_pending = self
            .text_highlights(HighlightKey::PendingInput, cx)
            .map(|(_, ranges)| ranges.to_vec());
        if existing_pending.is_none() && pending.is_empty() {
            return;
        }
        let transaction =
            self.transact(window, cx, |this, window, cx| {
                let selections = this
                    .selections
                    .all::<MultiBufferOffset>(&this.display_snapshot(cx));
                let edits = selections
                    .iter()
                    .map(|selection| (selection.end..selection.end, pending.clone()));
                this.edit(edits, cx);
                this.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                    s.select_ranges(selections.into_iter().enumerate().map(|(ix, sel)| {
                        sel.start + ix * pending.len()..sel.end + ix * pending.len()
                    }));
                });
                if let Some(existing_ranges) = existing_pending {
                    let edits = existing_ranges.iter().map(|range| (range.clone(), ""));
                    this.edit(edits, cx);
                }
            });

        let snapshot = self.snapshot(window, cx);
        let ranges = self
            .selections
            .all::<MultiBufferOffset>(&snapshot.display_snapshot)
            .into_iter()
            .map(|selection| {
                snapshot.buffer_snapshot().anchor_after(selection.end)
                    ..snapshot
                        .buffer_snapshot()
                        .anchor_before(selection.end + pending.len())
            })
            .collect();

        if pending.is_empty() {
            self.clear_highlights(HighlightKey::PendingInput, cx);
        } else {
            self.highlight_text(
                HighlightKey::PendingInput,
                ranges,
                HighlightStyle {
                    underline: Some(UnderlineStyle {
                        thickness: px(1.),
                        color: None,
                        wavy: false,
                    }),
                    ..Default::default()
                },
                cx,
            );
        }

        self.ime_transaction = self.ime_transaction.or(transaction);
        if let Some(transaction) = self.ime_transaction {
            self.buffer.update(cx, |buffer, cx| {
                buffer.group_until_transaction(transaction, cx);
            });
        }

        if self
            .text_highlights(HighlightKey::PendingInput, cx)
            .is_none()
        {
            self.ime_transaction.take();
        }
    }

    pub fn register_action_renderer(
        &mut self,
        listener: impl Fn(&Editor, &mut Window, &mut Context<Editor>) + 'static,
    ) -> Subscription {
        let id = self.next_editor_action_id.post_inc();
        self.editor_actions
            .borrow_mut()
            .insert(id, Box::new(listener));

        let editor_actions = self.editor_actions.clone();
        Subscription::new(move || {
            editor_actions.borrow_mut().remove(&id);
        })
    }

    pub fn register_action<A: Action>(
        &mut self,
        listener: impl Fn(&A, &mut Window, &mut App) + 'static,
    ) -> Subscription {
        let id = self.next_editor_action_id.post_inc();
        let listener = Arc::new(listener);
        self.editor_actions.borrow_mut().insert(
            id,
            Box::new(move |_, window, _| {
                let listener = listener.clone();
                window.on_action(TypeId::of::<A>(), move |action, phase, window, cx| {
                    let action = action.downcast_ref().unwrap();
                    if phase == DispatchPhase::Bubble {
                        listener(action, window, cx)
                    }
                })
            }),
        );

        let editor_actions = self.editor_actions.clone();
        Subscription::new(move || {
            editor_actions.borrow_mut().remove(&id);
        })
    }

    pub fn file_header_size(&self) -> u32 {
        FILE_HEADER_HEIGHT
    }

    pub fn restore(
        &mut self,
        revert_changes: HashMap<BufferId, Vec<(Range<text::Anchor>, Rope)>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.buffer().update(cx, |multi_buffer, cx| {
            for (buffer_id, changes) in revert_changes {
                if let Some(buffer) = multi_buffer.buffer(buffer_id) {
                    buffer.update(cx, |buffer, cx| {
                        buffer.edit(
                            changes
                                .into_iter()
                                .map(|(range, text)| (range, text.to_string())),
                            None,
                            cx,
                        );
                    });
                }
            }
        });
        let selections = self
            .selections
            .all::<MultiBufferOffset>(&self.display_snapshot(cx));
        self.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select(selections);
        });
    }

    pub fn to_pixel_point(
        &mut self,
        source: Anchor,
        editor_snapshot: &EditorSnapshot,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<gpui::Point<Pixels>> {
        let source_point = source.to_display_point(editor_snapshot);
        self.display_to_pixel_point(source_point, editor_snapshot, window, cx)
    }

    pub fn display_to_pixel_point(
        &mut self,
        source: DisplayPoint,
        editor_snapshot: &EditorSnapshot,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<gpui::Point<Pixels>> {
        let line_height = self.style(cx).text.line_height_in_pixels(window.rem_size());
        let text_layout_details = self.text_layout_details(window, cx);
        let scroll_top = text_layout_details
            .scroll_anchor
            .scroll_position(editor_snapshot)
            .y;

        if source.row().as_f64() < scroll_top.floor() {
            return None;
        }
        let source_x = editor_snapshot.x_for_display_point(source, &text_layout_details);
        let source_y = line_height * (source.row().as_f64() - scroll_top) as f32;
        Some(gpui::Point::new(source_x, source_y))
    }

    pub fn register_addon<T: Addon>(&mut self, instance: T) {
        self.addons
            .insert(std::any::TypeId::of::<T>(), Box::new(instance));
    }

    pub fn unregister_addon<T: Addon>(&mut self) {
        self.addons.remove(&std::any::TypeId::of::<T>());
    }

    pub fn addon<T: Addon>(&self) -> Option<&T> {
        let type_id = std::any::TypeId::of::<T>();
        self.addons
            .get(&type_id)
            .and_then(|item| item.to_any().downcast_ref::<T>())
    }

    pub fn addon_mut<T: Addon>(&mut self) -> Option<&mut T> {
        let type_id = std::any::TypeId::of::<T>();
        self.addons
            .get_mut(&type_id)
            .and_then(|item| item.to_any_mut()?.downcast_mut::<T>())
    }

    fn character_dimensions(&self, window: &mut Window, cx: &mut App) -> CharacterDimensions {
        let text_layout_details = self.text_layout_details(window, cx);
        let style = &text_layout_details.editor_style;
        let font_id = window.text_system().resolve_font(&style.text.font());
        let font_size = style.text.font_size.to_pixels(window.rem_size());
        let line_height = style.text.line_height_in_pixels(window.rem_size());
        let em_width = window.text_system().em_width(font_id, font_size).unwrap();
        let em_advance = window.text_system().em_advance(font_id, font_size).unwrap();

        CharacterDimensions {
            em_width,
            em_advance,
            line_height,
        }
    }

    fn create_style(&self, cx: &App) -> EditorStyle {
        let settings = ThemeSettings::get_global(cx);

        let mut text_style = match self.mode {
            EditorMode::SingleLine | EditorMode::AutoHeight { .. } => TextStyle {
                color: cx.theme().colors().editor_foreground,
                font_family: settings.ui_font.family.clone(),
                font_features: settings.ui_font.features.clone(),
                font_fallbacks: settings.ui_font.fallbacks.clone(),
                font_size: rems(0.875).into(),
                font_weight: settings.ui_font.weight,
                line_height: relative(settings.buffer_line_height.value()),
                ..Default::default()
            },
            EditorMode::Full { .. } => TextStyle {
                color: cx.theme().colors().editor_foreground,
                font_family: settings.buffer_font.family.clone(),
                font_features: settings.buffer_font.features.clone(),
                font_fallbacks: settings.buffer_font.fallbacks.clone(),
                font_size: settings.buffer_font_size(cx).into(),
                font_weight: settings.buffer_font.weight,
                line_height: relative(settings.buffer_line_height.value()),
                ..Default::default()
            },
        };
        if let Some(text_style_refinement) = &self.text_style_refinement {
            text_style.refine(text_style_refinement)
        }

        let background = match self.mode {
            EditorMode::SingleLine => cx.theme().system().transparent,
            EditorMode::AutoHeight { .. } => cx.theme().system().transparent,
            EditorMode::Full { .. } => cx.theme().colors().editor_background,
        };

        EditorStyle {
            background,
            border: cx.theme().colors().border,
            local_player: cx.theme().players().local(),
            text: text_style,
            scrollbar_width: EditorElement::SCROLLBAR_WIDTH,
            syntax: cx.theme().syntax().clone(),
            status: cx.theme().status().clone(),
            inlay_style: make_inlay_style(cx),
            edit_prediction_styles: make_suggestion_styles(cx),
            unnecessary_code_fade: settings.unnecessary_code_fade,
            show_underlines: false,
        }
    }

    pub fn disable_mouse_wheel_zoom(&mut self) {
        self.enable_mouse_wheel_zoom = false;
    }

    fn update_data_on_scroll(
        &mut self,
        debounce: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if debounce {
            self.post_scroll_update = cx.spawn_in(window, async move |editor, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                editor
                    .update_in(cx, |editor, window, cx| {
                        editor.do_update_data_on_scroll(window, cx);
                    })
                    .ok();
            });
        } else {
            self.post_scroll_update = Task::ready(());
            self.do_update_data_on_scroll(window, cx);
        }
    }

    fn do_update_data_on_scroll(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) {
        self.colorize_brackets(false, cx);
    }

    /// Returns the current cursor's vertical offset, in display rows, from the
    /// top of the visible viewport.
    /// Returns `None` if the cursor is not currently on screen.
    pub fn cursor_top_offset(&self, cx: &mut Context<Self>) -> Option<ScrollOffset> {
        let visible = self.visible_line_count()?;
        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let scroll_top = self.scroll_manager.scroll_position(&display_map, cx).y;
        let cursor_display_row = self
            .selections
            .newest::<Point>(&display_map)
            .head()
            .to_display_point(&display_map)
            .row()
            .as_f64();

        match cursor_display_row - scroll_top {
            offset if offset < 0.0 || offset >= visible => None,
            offset => Some(offset),
        }
    }
}

impl Editor {
    pub fn transact(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>),
    ) -> Option<TransactionId> {
        self.with_selection_effects_deferred(window, cx, |this, window, cx| {
            this.start_transaction_at(Instant::now(), window, cx);
            update(this, window, cx);
            this.end_transaction_at(Instant::now(), cx)
        })
    }

    pub fn start_transaction_at(
        &mut self,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<TransactionId> {
        self.end_selection(window, cx);
        if let Some(transaction_id) = self
            .buffer
            .update(cx, |buffer, cx| buffer.start_transaction_at(now, cx))
        {
            self.selection_history
                .insert_transaction(transaction_id, self.selections.disjoint_anchors_arc());
            cx.emit(EditorEvent::TransactionBegun { transaction_id });
            Some(transaction_id)
        } else {
            None
        }
    }

    pub fn end_transaction_at(
        &mut self,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Option<TransactionId> {
        if let Some(transaction_id) = self
            .buffer
            .update(cx, |buffer, cx| buffer.end_transaction_at(now, cx))
        {
            if let Some((_, end_selections)) =
                self.selection_history.transaction_mut(transaction_id)
            {
                *end_selections = Some(self.selections.disjoint_anchors_arc());
            } else {
                log::error!("unexpectedly ended a transaction that was not started by this editor");
            }

            cx.emit(EditorEvent::Edited { transaction_id });
            Some(transaction_id)
        } else {
            None
        }
    }

    pub fn finalize_last_transaction(&mut self, cx: &mut Context<Self>) {
        self.buffer
            .update(cx, |buffer, cx| buffer.finalize_last_transaction(cx));
    }

    pub fn group_until_transaction(
        &mut self,
        transaction_id: TransactionId,
        cx: &mut Context<Self>,
    ) {
        self.buffer.update(cx, |buffer, cx| {
            buffer.group_until_transaction(transaction_id, cx)
        });
    }

    pub fn modify_transaction_selection_history(
        &mut self,
        transaction_id: TransactionId,
        modify: impl FnOnce(&mut (Arc<[Selection<Anchor>]>, Option<Arc<[Selection<Anchor>]>>)),
    ) -> bool {
        self.selection_history
            .transaction_mut(transaction_id)
            .map(modify)
            .is_some()
    }

    pub fn set_mark(&mut self, _: &actions::SetMark, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection_mark_mode {
            self.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                selections.move_with(&mut |_, selection| {
                    selection.collapse_to(selection.head(), SelectionGoal::None);
                });
            });
        }
        self.selection_mark_mode = true;
        cx.notify();
    }

    pub fn swap_selection_ends(
        &mut self,
        _: &actions::SwapSelectionEnds,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
            selections.move_with(&mut |_, selection| {
                if selection.start != selection.end {
                    selection.reversed = !selection.reversed;
                }
            });
        });
        self.request_autoscroll(Autoscroll::newest(), cx);
        cx.notify();
    }

    pub fn toggle_focus(
        workspace: &mut Workspace,
        _: &actions::ToggleFocus,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let Some(item) = workspace.recent_active_item_by_type::<Self>(cx) else {
            return;
        };
        workspace.activate_item(&item, true, true, window, cx);
    }

    pub fn toggle_fold(
        &mut self,
        _: &actions::ToggleFold,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.buffer_kind(cx) == ItemBufferKind::Singleton {
            let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
            let selection = self.selections.newest::<Point>(&display_map);
            let range = if selection.is_empty() {
                let point = selection.head().to_display_point(&display_map);
                let start = DisplayPoint::new(point.row(), 0).to_point(&display_map);
                let end = DisplayPoint::new(point.row(), display_map.line_len(point.row()))
                    .to_point(&display_map);
                start..end
            } else {
                selection.range()
            };

            if display_map.folds_in_range(range).next().is_some() {
                self.unfold_lines(&Default::default(), window, cx);
            } else {
                self.fold(&Default::default(), window, cx);
            }
        } else {
            let multi_buffer_snapshot = self.buffer.read(cx).snapshot(cx);
            let buffer_ids = self
                .selections
                .disjoint_anchor_ranges()
                .flat_map(|range| multi_buffer_snapshot.buffer_ids_for_range(range))
                .collect::<HashSet<_>>();
            let should_unfold = buffer_ids
                .iter()
                .any(|buffer_id| self.is_buffer_folded(*buffer_id, cx));

            for buffer_id in buffer_ids {
                if should_unfold {
                    self.unfold_buffer(buffer_id, cx);
                } else {
                    self.fold_buffer(buffer_id, cx);
                }
            }
        }
    }

    pub fn toggle_fold_recursive(
        &mut self,
        _: &actions::ToggleFoldRecursive,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let selection = self.selections.newest::<Point>(&display_map);
        let range = if selection.is_empty() {
            let point = selection.head().to_display_point(&display_map);
            let start = DisplayPoint::new(point.row(), 0).to_point(&display_map);
            let end = DisplayPoint::new(point.row(), display_map.line_len(point.row()))
                .to_point(&display_map);
            start..end
        } else {
            selection.range()
        };

        if display_map.folds_in_range(range).next().is_some() {
            self.unfold_recursive(&Default::default(), window, cx);
        } else {
            self.fold_recursive(&Default::default(), window, cx);
        }
    }

    pub fn fold(&mut self, _: &actions::Fold, window: &mut Window, cx: &mut Context<Self>) {
        if self.buffer_kind(cx) == ItemBufferKind::Singleton {
            let mut creases = Vec::new();
            let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));

            for selection in self.selections.all_adjusted(&display_map) {
                let range = selection.range().sorted();
                let buffer_start_row = range.start.row;

                if range.start.row != range.end.row {
                    let mut found = false;
                    let mut row = range.start.row;
                    while row <= range.end.row {
                        if let Some(crease) = display_map.crease_for_buffer_row(MultiBufferRow(row))
                        {
                            found = true;
                            row = crease.range().end.row + 1;
                            creases.push(crease);
                        } else {
                            row += 1;
                        }
                    }
                    if found {
                        continue;
                    }
                }

                for row in (0..=range.start.row).rev() {
                    if let Some(crease) = display_map.crease_for_buffer_row(MultiBufferRow(row))
                        && crease.range().end.row >= buffer_start_row
                    {
                        creases.push(crease);
                        break;
                    }
                }
            }

            self.fold_creases(creases, true, window, cx);
        } else {
            let multi_buffer_snapshot = self.buffer.read(cx).snapshot(cx);
            let buffer_ids = self
                .selections
                .disjoint_anchor_ranges()
                .flat_map(|range| multi_buffer_snapshot.buffer_ids_for_range(range))
                .collect::<HashSet<_>>();
            for buffer_id in buffer_ids {
                self.fold_buffer(buffer_id, cx);
            }
        }
    }

    pub fn toggle_fold_all(
        &mut self,
        _: &actions::ToggleFoldAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let has_folds = if self.buffer.read(cx).is_singleton() {
            let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
            display_map
                .folds_in_range(MultiBufferOffset(0)..display_map.buffer_snapshot().len())
                .next()
                .is_some()
        } else {
            let snapshot = self.buffer.read(cx).snapshot(cx);
            snapshot
                .all_buffer_ids()
                .any(|buffer_id| self.is_buffer_folded(buffer_id, cx))
        };

        if has_folds {
            self.unfold_all(&actions::UnfoldAll, window, cx);
        } else {
            self.fold_all(&actions::FoldAll, window, cx);
        }
    }

    fn fold_at_level(
        &mut self,
        fold_at: &FoldAtLevel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.buffer.read(cx).is_singleton() {
            return;
        }

        let fold_at_level = fold_at.0;
        let snapshot = self.buffer.read(cx).snapshot(cx);
        let row_ranges_to_keep = self
            .selections
            .all::<Point>(&self.display_snapshot(cx))
            .into_iter()
            .map(|selection| selection.start.row..selection.end.row)
            .collect::<Vec<_>>();
        let mut creases = Vec::new();
        let mut stack = vec![(0, snapshot.max_row().0, 1)];

        while let Some((mut start_row, end_row, current_level)) = stack.pop() {
            while start_row < end_row {
                match self
                    .snapshot(window, cx)
                    .crease_for_buffer_row(MultiBufferRow(start_row))
                {
                    Some(crease) => {
                        let nested_start_row = crease.range().start.row + 1;
                        let nested_end_row = crease.range().end.row;

                        if current_level < fold_at_level {
                            stack.push((nested_start_row, nested_end_row, current_level + 1));
                        } else if current_level == fold_at_level
                            && !row_ranges_to_keep.iter().any(|selection| {
                                selection.end >= nested_start_row
                                    && selection.start <= nested_end_row
                            })
                        {
                            creases.push(crease);
                        }

                        start_row = nested_end_row + 1;
                    }
                    None => start_row += 1,
                }
            }
        }

        self.fold_creases(creases, true, window, cx);
    }

    pub fn fold_at_level_1(
        &mut self,
        _: &actions::FoldAtLevel1,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fold_at_level(&actions::FoldAtLevel(1), window, cx);
    }

    pub fn fold_at_level_2(
        &mut self,
        _: &actions::FoldAtLevel2,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fold_at_level(&actions::FoldAtLevel(2), window, cx);
    }

    pub fn fold_at_level_3(
        &mut self,
        _: &actions::FoldAtLevel3,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fold_at_level(&actions::FoldAtLevel(3), window, cx);
    }

    pub fn fold_at_level_4(
        &mut self,
        _: &actions::FoldAtLevel4,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fold_at_level(&actions::FoldAtLevel(4), window, cx);
    }

    pub fn fold_at_level_5(
        &mut self,
        _: &actions::FoldAtLevel5,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fold_at_level(&actions::FoldAtLevel(5), window, cx);
    }

    pub fn fold_at_level_6(
        &mut self,
        _: &actions::FoldAtLevel6,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fold_at_level(&actions::FoldAtLevel(6), window, cx);
    }

    pub fn fold_at_level_7(
        &mut self,
        _: &actions::FoldAtLevel7,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fold_at_level(&actions::FoldAtLevel(7), window, cx);
    }

    pub fn fold_at_level_8(
        &mut self,
        _: &actions::FoldAtLevel8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fold_at_level(&actions::FoldAtLevel(8), window, cx);
    }

    pub fn fold_at_level_9(
        &mut self,
        _: &actions::FoldAtLevel9,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fold_at_level(&actions::FoldAtLevel(9), window, cx);
    }

    pub fn fold_all(&mut self, _: &actions::FoldAll, window: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.read(cx).is_singleton() {
            let snapshot = self.buffer.read(cx).snapshot(cx);
            let mut creases = Vec::new();
            for row in 0..snapshot.max_row().0 {
                if let Some(crease) = self
                    .snapshot(window, cx)
                    .crease_for_buffer_row(MultiBufferRow(row))
                {
                    creases.push(crease);
                }
            }
            self.fold_creases(creases, true, window, cx);
        } else {
            let snapshot = self.buffer.read(cx).snapshot(cx);
            for buffer_id in snapshot.all_buffer_ids().collect::<Vec<_>>() {
                self.fold_buffer(buffer_id, cx);
            }
        }
    }

    pub fn fold_function_bodies(
        &mut self,
        _: &actions::FoldFunctionBodies,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let snapshot = self.buffer.read(cx).snapshot(cx);
        let creases = snapshot
            .text_object_ranges(
                MultiBufferOffset(0)..snapshot.len(),
                TreeSitterOptions::default(),
            )
            .filter_map(|(range, object)| (object == TextObject::InsideFunction).then_some(range))
            .map(|range| Crease::simple(range, self.display_map.read(cx).fold_placeholder.clone()))
            .collect::<Vec<_>>();
        self.fold_creases(creases, true, window, cx);
    }

    pub fn fold_recursive(
        &mut self,
        _: &actions::FoldRecursive,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let mut creases = Vec::new();

        for selection in self.selections.all_adjusted(&display_map) {
            let range = selection.range().sorted();
            let buffer_start_row = range.start.row;

            if range.start.row != range.end.row {
                let mut found = false;
                for row in range.start.row..=range.end.row {
                    if let Some(crease) = display_map.crease_for_buffer_row(MultiBufferRow(row)) {
                        found = true;
                        creases.push(crease);
                    }
                }
                if found {
                    continue;
                }
            }

            for row in (0..=range.start.row).rev() {
                if let Some(crease) = display_map.crease_for_buffer_row(MultiBufferRow(row)) {
                    if crease.range().end.row >= buffer_start_row {
                        creases.push(crease);
                    } else {
                        break;
                    }
                }
            }
        }

        self.fold_creases(creases, true, window, cx);
    }

    pub fn fold_at(
        &mut self,
        buffer_row: MultiBufferRow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        if let Some(crease) = display_map.crease_for_buffer_row(buffer_row) {
            let autoscroll = self
                .selections
                .all::<Point>(&display_map)
                .iter()
                .any(|selection| crease.range().overlaps(&selection.range()));
            self.fold_creases(vec![crease], autoscroll, window, cx);
        }
    }

    pub fn unfold_lines(&mut self, _: &UnfoldLines, _window: &mut Window, cx: &mut Context<Self>) {
        if self.buffer_kind(cx) == ItemBufferKind::Singleton {
            let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
            let buffer = display_map.buffer_snapshot();
            let ranges = self
                .selections
                .all::<Point>(&display_map)
                .iter()
                .map(|selection| {
                    let range = selection.display_range(&display_map).sorted();
                    let mut start = range.start.to_point(&display_map);
                    let mut end = range.end.to_point(&display_map);
                    start.column = 0;
                    end.column = buffer.line_len(MultiBufferRow(end.row));
                    start..end
                })
                .collect::<Vec<_>>();

            self.unfold_ranges(&ranges, true, true, cx);
        } else {
            let multi_buffer_snapshot = self.buffer.read(cx).snapshot(cx);
            let buffer_ids = self
                .selections
                .disjoint_anchor_ranges()
                .flat_map(|range| multi_buffer_snapshot.buffer_ids_for_range(range))
                .collect::<HashSet<_>>();
            for buffer_id in buffer_ids {
                self.unfold_buffer(buffer_id, cx);
            }
        }
    }

    pub fn unfold_recursive(
        &mut self,
        _: &UnfoldRecursive,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let ranges = self
            .selections
            .all::<Point>(&display_map)
            .iter()
            .map(|selection| {
                let mut range = selection.display_range(&display_map).sorted();
                *range.start.column_mut() = 0;
                *range.end.column_mut() = display_map.line_len(range.end.row());
                range.start.to_point(&display_map)..range.end.to_point(&display_map)
            })
            .collect::<Vec<_>>();

        self.unfold_ranges(&ranges, true, true, cx);
    }

    pub fn unfold_at(
        &mut self,
        buffer_row: MultiBufferRow,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let intersection_range = Point::new(buffer_row.0, 0)
            ..Point::new(
                buffer_row.0,
                display_map.buffer_snapshot().line_len(buffer_row),
            );
        let autoscroll = self
            .selections
            .all::<Point>(&display_map)
            .iter()
            .any(|selection| RangeExt::overlaps(&selection.range(), &intersection_range));
        self.unfold_ranges(&[intersection_range], true, autoscroll, cx);
    }

    pub fn unfold_all(
        &mut self,
        _: &actions::UnfoldAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.buffer.read(cx).is_singleton() {
            let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
            self.unfold_ranges(
                &[MultiBufferOffset(0)..display_map.buffer_snapshot().len()],
                true,
                true,
                cx,
            );
        } else {
            let snapshot = self.buffer.read(cx).snapshot(cx);
            for buffer_id in snapshot.all_buffer_ids().collect::<Vec<_>>() {
                self.unfold_buffer(buffer_id, cx);
            }
        }
    }

    pub fn fold_selected_ranges(
        &mut self,
        _: &FoldSelectedRanges,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let creases = self
            .selections
            .all_adjusted(&display_map)
            .into_iter()
            .map(|selection| {
                Crease::simple(selection.range(), display_map.fold_placeholder.clone())
            })
            .collect::<Vec<_>>();
        self.fold_creases(creases, true, window, cx);
    }

    pub fn fold_ranges<T: ToOffset + Clone>(
        &mut self,
        ranges: Vec<Range<T>>,
        auto_scroll: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display_map = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let creases = ranges
            .into_iter()
            .map(|range| Crease::simple(range, display_map.fold_placeholder.clone()))
            .collect::<Vec<_>>();
        self.fold_creases(creases, auto_scroll, window, cx);
    }

    pub fn fold_creases<T: ToOffset + Clone>(
        &mut self,
        creases: Vec<Crease<T>>,
        auto_scroll: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if creases.is_empty() {
            return;
        }

        self.display_map.update(cx, |map, cx| map.fold(creases, cx));
        if auto_scroll {
            self.request_autoscroll(Autoscroll::fit(), cx);
        }

        cx.notify();
        self.scrollbar_marker_state.dirty = true;
        self.update_data_on_scroll(false, window, cx);
    }

    pub fn unfold_ranges<T: ToOffset + Clone>(
        &mut self,
        ranges: &[Range<T>],
        inclusive: bool,
        auto_scroll: bool,
        cx: &mut Context<Self>,
    ) {
        self.remove_folds_with(ranges, auto_scroll, cx, |map, cx| {
            map.unfold_intersecting(ranges.iter().cloned(), inclusive, cx);
        });
    }

    pub fn fold_buffer(&mut self, buffer_id: BufferId, cx: &mut Context<Self>) {
        self.fold_buffers([buffer_id], cx);
    }

    pub fn fold_buffers(
        &mut self,
        buffer_ids: impl IntoIterator<Item = BufferId>,
        cx: &mut Context<Self>,
    ) {
        if self.buffer().read(cx).is_singleton() {
            return;
        }

        let ids_to_fold = buffer_ids
            .into_iter()
            .filter(|buffer_id| !self.is_buffer_folded(*buffer_id, cx))
            .collect::<Vec<_>>();
        if ids_to_fold.is_empty() {
            return;
        }

        self.display_map.update(cx, |display_map, cx| {
            display_map.fold_buffers(ids_to_fold.clone(), cx);
        });

        let snapshot = self.display_snapshot(cx);
        self.selections.change_with(&snapshot, |selections| {
            for buffer_id in ids_to_fold.iter().copied() {
                selections.remove_selections_from_buffer(buffer_id);
            }
        });

        cx.emit(EditorEvent::BufferFoldToggled {
            ids: ids_to_fold,
            folded: true,
        });
        cx.notify();
    }

    pub fn unfold_buffer(&mut self, buffer_id: BufferId, cx: &mut Context<Self>) {
        if self.buffer().read(cx).is_singleton() || !self.is_buffer_folded(buffer_id, cx) {
            return;
        }
        self.display_map.update(cx, |display_map, cx| {
            display_map.unfold_buffers([buffer_id], cx);
        });
        cx.emit(EditorEvent::BufferFoldToggled {
            ids: vec![buffer_id],
            folded: false,
        });
        cx.notify();
    }

    pub fn is_buffer_folded(&self, buffer_id: BufferId, cx: &App) -> bool {
        self.display_map.read(cx).is_buffer_folded(buffer_id)
    }

    pub fn has_any_buffer_folded(&self, cx: &App) -> bool {
        !self.buffer().read(cx).is_singleton() && !self.folded_buffers(cx).is_empty()
    }

    pub fn folded_buffers<'a>(&self, cx: &'a App) -> &'a HashSet<BufferId> {
        self.display_map.read(cx).folded_buffers()
    }

    pub fn disable_header_for_buffer(&mut self, buffer_id: BufferId, cx: &mut Context<Self>) {
        self.display_map.update(cx, |display_map, cx| {
            display_map.disable_header_for_buffer(buffer_id, cx);
        });
        cx.notify();
    }

    pub fn remove_folds_with_type<T: ToOffset + Clone>(
        &mut self,
        ranges: &[Range<T>],
        type_id: TypeId,
        auto_scroll: bool,
        cx: &mut Context<Self>,
    ) {
        self.remove_folds_with(ranges, auto_scroll, cx, |map, cx| {
            map.remove_folds_with_type(ranges.iter().cloned(), type_id, cx);
        });
    }

    fn remove_folds_with<T: ToOffset + Clone>(
        &mut self,
        ranges: &[Range<T>],
        auto_scroll: bool,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut DisplayMap, &mut Context<DisplayMap>),
    ) {
        if ranges.is_empty() {
            return;
        }

        self.display_map.update(cx, update);
        if auto_scroll {
            self.request_autoscroll(Autoscroll::fit(), cx);
        }

        cx.notify();
        self.scrollbar_marker_state.dirty = true;
        self.active_indent_guides_state.dirty = true;
    }

    pub fn update_renderer_widths(
        &mut self,
        widths: impl IntoIterator<Item = (ChunkRendererId, Pixels)>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.display_map
            .update(cx, |map, cx| map.update_fold_widths(widths, cx))
    }

    pub fn default_fold_placeholder(&self, cx: &App) -> FoldPlaceholder {
        self.display_map.read(cx).fold_placeholder.clone()
    }

    pub fn longest_row(&self, cx: &mut App) -> DisplayRow {
        self.display_map
            .update(cx, |map, cx| map.snapshot(cx))
            .longest_row()
    }

    pub fn max_point(&self, cx: &mut App) -> DisplayPoint {
        self.display_map
            .update(cx, |map, cx| map.snapshot(cx))
            .max_point()
    }

    pub fn text(&self, cx: &App) -> String {
        self.buffer.read(cx).read(cx).text()
    }

    pub fn is_empty(&self, cx: &App) -> bool {
        self.buffer.read(cx).read(cx).is_empty()
    }

    pub fn text_option(&self, cx: &App) -> Option<String> {
        let text = self.text(cx);
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    pub fn set_text(
        &mut self,
        text: impl Into<Arc<str>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.transact(window, cx, |this, _, cx| {
            if let Some(buffer) = this.buffer.read(cx).as_singleton() {
                buffer.update(cx, |buffer, cx| buffer.set_text(text, cx));
            }
        });
    }

    pub fn display_text(&self, cx: &mut App) -> String {
        self.display_map
            .update(cx, |map, cx| map.snapshot(cx))
            .text()
    }

    pub fn wrap_guides(&self, cx: &App) -> SmallVec<[(usize, bool); 2]> {
        let mut wrap_guides = smallvec![];
        if self.show_wrap_guides == Some(false) {
            return wrap_guides;
        }

        let settings = self.buffer.read(cx).language_settings(cx);
        if settings.show_wrap_guides {
            match self.soft_wrap_mode(cx) {
                SoftWrap::Bounded(soft_wrap) => wrap_guides.push((soft_wrap as usize, true)),
                SoftWrap::GitDiff | SoftWrap::None | SoftWrap::EditorWidth => {}
            }
            wrap_guides.extend(settings.wrap_guides.iter().map(|guide| (*guide, false)));
        }

        wrap_guides
    }

    pub fn soft_wrap_mode(&self, cx: &App) -> SoftWrap {
        let settings = self.buffer.read(cx).language_settings(cx);
        match self.soft_wrap_mode_override.unwrap_or(settings.soft_wrap) {
            language_settings::SoftWrap::PreferLine | language_settings::SoftWrap::None => {
                SoftWrap::None
            }
            language_settings::SoftWrap::EditorWidth => SoftWrap::EditorWidth,
            language_settings::SoftWrap::Bounded => {
                SoftWrap::Bounded(settings.preferred_line_length)
            }
        }
    }

    pub fn set_soft_wrap_mode(
        &mut self,
        mode: language_settings::SoftWrap,
        cx: &mut Context<Self>,
    ) {
        self.soft_wrap_mode_override = Some(mode);
        cx.notify();
    }

    pub fn set_hard_wrap(&mut self, hard_wrap: Option<usize>, cx: &mut Context<Self>) {
        self.hard_wrap = hard_wrap;
        cx.notify();
    }

    pub fn set_text_style_refinement(&mut self, style: TextStyleRefinement) {
        self.text_style_refinement = Some(style);
    }

    pub fn set_style(&mut self, style: EditorStyle, window: &mut Window, cx: &mut Context<Self>) {
        let font = style.text.font();
        let font_size = style.text.font_size.to_pixels(window.rem_size());
        let display_map = self
            .placeholder_display_map
            .as_ref()
            .filter(|_| self.is_empty(cx))
            .unwrap_or(&self.display_map);

        display_map.update(cx, |map, cx| map.set_font(font, font_size, cx));
        self.style = Some(style);
    }

    pub fn style(&mut self, cx: &App) -> &EditorStyle {
        if self.style.is_none() {
            self.style = Some(self.create_style(cx));
        }
        self.style
            .as_ref()
            .expect("editor style initialized immediately above")
    }

    pub(crate) fn set_wrap_width(&self, width: Option<Pixels>, cx: &mut App) -> bool {
        if self.is_empty(cx) {
            self.placeholder_display_map
                .as_ref()
                .is_some_and(|display_map| {
                    display_map.update(cx, |map, cx| map.set_wrap_width(width, cx))
                })
        } else {
            self.display_map
                .update(cx, |map, cx| map.set_wrap_width(width, cx))
        }
    }

    pub fn set_soft_wrap(&mut self) {
        self.soft_wrap_mode_override = Some(language_settings::SoftWrap::EditorWidth);
    }

    pub fn toggle_soft_wrap(&mut self, _: &ToggleSoftWrap, _: &mut Window, cx: &mut Context<Self>) {
        if self.soft_wrap_mode_override.is_some() {
            self.soft_wrap_mode_override.take();
        } else {
            let soft_wrap = match self.soft_wrap_mode(cx) {
                SoftWrap::GitDiff => return,
                SoftWrap::None => language_settings::SoftWrap::EditorWidth,
                SoftWrap::EditorWidth | SoftWrap::Bounded(_) => language_settings::SoftWrap::None,
            };
            self.soft_wrap_mode_override = Some(soft_wrap);
        }
        cx.notify();
    }

    pub fn toggle_tab_bar(&mut self, _: &ToggleTabBar, _: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace() else {
            return;
        };
        let fs = workspace.read(cx).app_state().fs.clone();
        let current_show = TabBarSettings::get_global(cx).show;
        update_settings_file(fs, cx, move |settings, _| {
            settings.tab_bar.get_or_insert_default().show = Some(!current_show);
        });
    }

    pub fn toggle_indent_guides(
        &mut self,
        _: &ToggleIndentGuides,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let currently_enabled = self.should_show_indent_guides().unwrap_or_else(|| {
            self.buffer
                .read(cx)
                .language_settings(cx)
                .indent_guides
                .enabled
        });
        self.show_indent_guides = Some(!currently_enabled);
        cx.notify();
    }

    fn should_show_indent_guides(&self) -> Option<bool> {
        self.show_indent_guides
    }

    pub fn disable_indent_guides_for_buffer(
        &mut self,
        buffer_id: BufferId,
        cx: &mut Context<Self>,
    ) {
        self.buffers_with_disabled_indent_guides.insert(buffer_id);
        cx.notify();
    }

    pub fn has_indent_guides_disabled_for_buffer(&self, buffer_id: BufferId) -> bool {
        self.buffers_with_disabled_indent_guides
            .contains(&buffer_id)
    }

    pub fn toggle_line_numbers(
        &mut self,
        _: &ToggleLineNumbers,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_line_numbers = Some(!self.line_numbers_enabled(cx));
        cx.notify();
    }

    pub fn line_numbers_enabled(&self, cx: &App) -> bool {
        self.show_line_numbers
            .unwrap_or_else(|| EditorSettings::get_global(cx).gutter.line_numbers)
    }

    pub fn relative_line_numbers(&self, cx: &App) -> RelativeLineNumbers {
        match (
            self.use_relative_line_numbers,
            EditorSettings::get_global(cx).relative_line_numbers,
        ) {
            (None, setting) => setting,
            (Some(false), _) => RelativeLineNumbers::Disabled,
            (Some(true), RelativeLineNumbers::Wrapped) => RelativeLineNumbers::Wrapped,
            (Some(true), _) => RelativeLineNumbers::Enabled,
        }
    }

    pub fn toggle_relative_line_numbers(
        &mut self,
        _: &ToggleRelativeLineNumbers,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_relative = self.relative_line_numbers(cx);
        self.set_relative_line_number(Some(!is_relative.enabled()), cx);
    }

    pub fn set_relative_line_number(&mut self, is_relative: Option<bool>, cx: &mut Context<Self>) {
        self.use_relative_line_numbers = is_relative;
        cx.notify();
    }

    pub fn set_show_gutter(&mut self, show_gutter: bool, cx: &mut Context<Self>) {
        self.show_gutter = show_gutter;
        cx.notify();
    }

    pub fn set_show_scrollbars(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_scrollbars = ScrollbarAxes {
            horizontal: show,
            vertical: show,
        };
        cx.notify();
    }

    pub fn set_show_vertical_scrollbar(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_scrollbars.vertical = show;
        cx.notify();
    }

    pub fn set_show_horizontal_scrollbar(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_scrollbars.horizontal = show;
        cx.notify();
    }

    pub fn set_offset_content(&mut self, offset_content: bool, cx: &mut Context<Self>) {
        self.offset_content = offset_content;
        cx.notify();
    }

    pub fn set_show_line_numbers(&mut self, show_line_numbers: bool, cx: &mut Context<Self>) {
        self.show_line_numbers = Some(show_line_numbers);
        cx.notify();
    }

    pub fn disable_expand_excerpt_buttons(&mut self, cx: &mut Context<Self>) {
        self.disable_expand_excerpt_buttons = true;
        cx.notify();
    }

    pub fn set_delegate_expand_excerpts(&mut self, delegate: bool) {
        self.delegate_expand_excerpts = delegate;
    }

    pub fn set_delegate_open_excerpts(&mut self, delegate: bool) {
        self.delegate_open_excerpts = delegate;
    }

    pub fn set_on_local_selections_changed(
        &mut self,
        callback: Option<Box<dyn Fn(Point, &mut Window, &mut Context<Self>) + 'static>>,
    ) {
        self.on_local_selections_changed = callback;
    }

    pub fn set_suppress_selection_callback(&mut self, suppress: bool) {
        self.suppress_selection_callback = suppress;
    }

    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        self.display_map.update(cx, |map, cx| {
            map.masked = masked;
            cx.notify();
        });
        cx.notify();
    }

    fn target_file<'a>(&self, cx: &'a App) -> Option<&'a dyn language::LocalFile> {
        let buffer = self.buffer.read(cx).as_singleton()?;
        buffer.read(cx).file()?.as_local()
    }

    pub fn target_file_abs_path(&self, cx: &mut Context<Self>) -> Option<PathBuf> {
        Some(self.target_file(cx)?.abs_path(cx))
    }

    pub fn working_directory(&self, cx: &App) -> Option<PathBuf> {
        self.target_file(cx)
            .and_then(|file| file.abs_path(cx).parent().map(Path::to_path_buf))
    }

    pub fn project_path(&self, cx: &App) -> Option<ProjectPath> {
        let file = self
            .buffer
            .read(cx)
            .as_singleton()?
            .read(cx)
            .file()?
            .as_local()?;
        Some(ProjectPath {
            worktree_id: file.worktree_id(cx),
            path: file.path().clone(),
        })
    }
}

impl Editor {
    pub fn copy_and_trim(&mut self, _: &CopyAndTrim, _: &mut Window, cx: &mut Context<Self>) {
        self.do_copy(true, cx);
    }

    pub fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.do_copy(false, cx);
    }

    fn do_copy(&self, strip_leading_indents: bool, cx: &mut Context<Self>) {
        let selections = self.selections.all::<Point>(&self.display_snapshot(cx));
        let buffer = self.buffer.read(cx).read(cx);
        let mut text = String::new();
        let max_point = buffer.max_point();
        let mut is_first = true;

        for selection in selections {
            let mut start = selection.start;
            let mut end = selection.end;
            let is_entire_line = selection.is_empty() || self.selections.line_mode();
            let mut add_trailing_newline = false;
            if is_entire_line {
                start = Point::new(start.row, 0);
                let next_line_start = Point::new(end.row + 1, 0);
                if next_line_start <= max_point {
                    end = next_line_start;
                } else {
                    end = Point::new(end.row, buffer.line_len(MultiBufferRow(end.row)));
                    add_trailing_newline = true;
                }
            }

            if !is_first {
                text.push('\n');
            }
            is_first = false;

            let copied = buffer.text_for_range(start..end).collect::<String>();
            if strip_leading_indents {
                text.push_str(copied.trim());
            } else {
                text.push_str(&copied);
            }
            if add_trailing_newline {
                text.push('\n');
            }
        }

        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    pub fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }
        self.copy(&Copy, window, cx);
        self.insert("", window, cx);
    }

    pub fn kill_ring_cut(&mut self, _: &KillRingCut, window: &mut Window, cx: &mut Context<Self>) {
        self.cut(&Cut, window, cx);
    }

    pub fn kill_ring_yank(
        &mut self,
        _: &KillRingYank,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.paste(&Paste, window, cx);
    }

    pub fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }

        if let Some(item) = cx.read_from_clipboard() {
            self.do_paste(&item.text().unwrap_or_default(), None, true, window, cx);
        }
    }

    pub fn paste_as_agent(
        &mut self,
        _: &zen_actions::editor::PasteAsAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only(cx) {
            return;
        }

        if let Some(item) = cx.read_from_clipboard() {
            let text = item.text().unwrap_or_default();
            self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
            self.replace_selections_with_authorship(
                &text,
                None,
                AuthorshipSource::Agent,
                window,
                cx,
            );
        }
    }

    pub fn do_paste(
        &mut self,
        text: &String,
        _clipboard_selections: Option<Vec<ClipboardSelection>>,
        _handle_entire_lines: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only(cx) {
            return;
        }
        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        self.transact(window, cx, |this, window, cx| {
            this.insert(text, window, cx);
        });
    }

    pub fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }
        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        if let Some(transaction_id) = self.buffer.update(cx, |buffer, cx| buffer.undo(cx)) {
            if let Some((selections, _)) =
                self.selection_history.transaction(transaction_id).cloned()
            {
                self.change_selections(
                    SelectionEffects::no_scroll(),
                    window,
                    cx,
                    |selection_set| {
                        selection_set.select_anchors(selections.to_vec());
                    },
                );
            }
            self.request_autoscroll(Autoscroll::fit(), cx);
            self.unmark_text(window, cx);
            self.refresh_edit_prediction(true, false, window, cx);
            cx.emit(EditorEvent::Edited { transaction_id });
            cx.emit(EditorEvent::TransactionUndone { transaction_id });
        }
    }

    pub fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }
        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        if let Some(transaction_id) = self.buffer.update(cx, |buffer, cx| buffer.redo(cx)) {
            if let Some((_, Some(selections))) =
                self.selection_history.transaction(transaction_id).cloned()
            {
                self.change_selections(
                    SelectionEffects::no_scroll(),
                    window,
                    cx,
                    |selection_set| {
                        selection_set.select_anchors(selections.to_vec());
                    },
                );
            }
            self.request_autoscroll(Autoscroll::fit(), cx);
            self.unmark_text(window, cx);
            self.refresh_edit_prediction(true, false, window, cx);
            cx.emit(EditorEvent::Edited { transaction_id });
        }
    }

    pub fn expand_excerpts(
        &mut self,
        action: &ExpandExcerpts,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.expand_excerpts_for_direction(action.lines, ExpandExcerptDirection::UpAndDown, cx);
    }

    pub fn expand_excerpts_down(
        &mut self,
        action: &ExpandExcerptsDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.expand_excerpts_for_direction(action.lines, ExpandExcerptDirection::Down, cx);
    }

    pub fn expand_excerpts_up(
        &mut self,
        action: &ExpandExcerptsUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.expand_excerpts_for_direction(action.lines, ExpandExcerptDirection::Up, cx);
    }

    pub fn expand_excerpts_for_direction(
        &mut self,
        lines: u32,
        direction: ExpandExcerptDirection,
        cx: &mut Context<Self>,
    ) {
        let lines = if lines == 0 {
            EditorSettings::get_global(cx).expand_excerpt_lines
        } else {
            lines
        };
        let snapshot = self.buffer.read(cx).snapshot(cx);
        let excerpt_anchors = self
            .selections
            .disjoint_anchor_ranges()
            .flat_map(|range| {
                snapshot
                    .range_to_buffer_ranges(range)
                    .into_iter()
                    .filter_map(|(buffer_snapshot, range, _)| {
                        snapshot.anchor_in_excerpt(buffer_snapshot.anchor_after(range.start))
                    })
            })
            .collect::<Vec<_>>();

        if self.delegate_expand_excerpts {
            cx.emit(EditorEvent::ExpandExcerptsRequested {
                excerpt_anchors,
                lines,
                direction,
            });
            return;
        }

        self.buffer.update(cx, |buffer, cx| {
            buffer.expand_excerpts(excerpt_anchors, lines, direction, cx);
        });
    }

    pub(crate) fn expand_excerpt(
        &mut self,
        excerpt_anchor: Anchor,
        direction: ExpandExcerptDirection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lines = EditorSettings::get_global(cx).expand_excerpt_lines;
        if self.delegate_expand_excerpts {
            cx.emit(EditorEvent::ExpandExcerptsRequested {
                excerpt_anchors: vec![excerpt_anchor],
                lines,
                direction,
            });
            return;
        }
        self.buffer.update(cx, |buffer, cx| {
            buffer.expand_excerpts([excerpt_anchor], lines, direction, cx);
        });
    }

    pub(crate) fn navigate_to_hover_links(
        &mut self,
        links: Vec<HoverLink>,
        _split: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Navigated>> {
        if let Some(HoverLink::Url(url)) = links.into_iter().next() {
            cx.open_url(&url);
            Task::ready(Ok(Navigated::Yes))
        } else {
            Task::ready(Ok(Navigated::No))
        }
    }

    pub fn reveal_in_finder(
        &mut self,
        _: &RevealInFileManager,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.target_file_abs_path(cx) {
            cx.reveal_path(&path);
        }
    }

    pub fn copy_path(
        &mut self,
        _: &zen_actions::workspace::CopyPath,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.target_file_abs_path(cx)
            && let Some(path) = path.to_str()
        {
            cx.write_to_clipboard(ClipboardItem::new_string(path.to_string()));
        } else {
            cx.propagate();
        }
    }

    pub fn copy_relative_path(
        &mut self,
        _: &zen_actions::workspace::CopyRelativePath,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.active_buffer(cx).and_then(|buffer| {
            let project = self.project()?.read(cx);
            let buffer = buffer.read(cx);
            let file = buffer.file()?;
            Some(file.path().display(project.path_style(cx)).to_string())
        }) {
            cx.write_to_clipboard(ClipboardItem::new_string(path));
        } else {
            cx.propagate();
        }
    }

    pub fn copy_file_name_without_extension(
        &mut self,
        _: &CopyFileNameWithoutExtension,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(file_stem) = self.active_buffer(cx).and_then(|buffer| {
            let file = buffer.read(cx).file()?;
            file.path().file_stem().map(|stem| stem.to_string())
        }) {
            cx.write_to_clipboard(ClipboardItem::new_string(file_stem));
        }
    }

    pub fn copy_file_name(&mut self, _: &CopyFileName, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(file_name) = self.active_buffer(cx).and_then(|buffer| {
            let file = buffer.read(cx).file()?;
            Some(file.file_name(cx).to_string())
        }) {
            cx.write_to_clipboard(ClipboardItem::new_string(file_name));
        }
    }

    pub fn copy_file_location(
        &mut self,
        _: &CopyFileLocation,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selection = self.selections.newest::<Point>(&self.display_snapshot(cx));
        let start_line = selection.start.row + 1;
        let end_line = selection.end.row + 1;
        if let Some(path) = self.active_buffer(cx).and_then(|buffer| {
            let project = self.project()?.read(cx);
            let buffer = buffer.read(cx);
            let file = buffer.file()?;
            Some(file.path().display(project.path_style(cx)).to_string())
        }) {
            let location = if start_line == end_line {
                format!("{path}:{start_line}")
            } else {
                format!("{path}:{start_line}-{end_line}")
            };
            cx.write_to_clipboard(ClipboardItem::new_string(location));
        }
    }

    pub fn go_to_singleton_buffer_point(
        &mut self,
        point: Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(buffer) = self.buffer.read(cx).as_singleton() else {
            return;
        };
        let Some(anchor) = self
            .buffer
            .read(cx)
            .buffer_point_to_anchor(&buffer, point, cx)
        else {
            return;
        };

        self.change_selections(
            SelectionEffects::scroll(Autoscroll::center()),
            window,
            cx,
            |selections| {
                selections.select_anchor_ranges([anchor..anchor]);
            },
        );
    }

    pub fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    pub fn move_selection_on_drop(
        &mut self,
        _selection: &Selection<Anchor>,
        _target: DisplayPoint,
        _is_cut: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    pub fn set_gutter_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered != self.gutter_hovered {
            self.gutter_hovered = hovered;
            cx.notify();
        }
    }

    pub fn insert_blocks(
        &mut self,
        blocks: impl IntoIterator<Item = BlockProperties<Anchor>>,
        autoscroll: Option<Autoscroll>,
        cx: &mut Context<Self>,
    ) -> Vec<CustomBlockId> {
        let block_ids = self
            .display_map
            .update(cx, |display_map, cx| display_map.insert_blocks(blocks, cx));
        if let Some(autoscroll) = autoscroll {
            self.request_autoscroll(autoscroll, cx);
        }
        cx.notify();
        block_ids
    }

    pub fn resize_blocks(
        &mut self,
        heights: HashMap<CustomBlockId, u32>,
        autoscroll: Option<Autoscroll>,
        cx: &mut Context<Self>,
    ) {
        self.display_map
            .update(cx, |display_map, cx| display_map.resize_blocks(heights, cx));
        if let Some(autoscroll) = autoscroll {
            self.request_autoscroll(autoscroll, cx);
        }
        cx.notify();
    }

    pub fn replace_blocks(
        &mut self,
        renderers: HashMap<CustomBlockId, RenderBlock>,
        autoscroll: Option<Autoscroll>,
        cx: &mut Context<Self>,
    ) {
        self.display_map
            .update(cx, |display_map, _| display_map.replace_blocks(renderers));
        if let Some(autoscroll) = autoscroll {
            self.request_autoscroll(autoscroll, cx);
        }
        cx.notify();
    }

    pub fn remove_blocks(
        &mut self,
        block_ids: HashSet<CustomBlockId>,
        autoscroll: Option<Autoscroll>,
        cx: &mut Context<Self>,
    ) {
        self.display_map.update(cx, |display_map, cx| {
            display_map.remove_blocks(block_ids, cx);
        });
        if let Some(autoscroll) = autoscroll {
            self.request_autoscroll(autoscroll, cx);
        }
        cx.notify();
    }

    pub fn row_for_block(
        &self,
        block_id: CustomBlockId,
        cx: &mut Context<Self>,
    ) -> Option<DisplayRow> {
        self.display_map
            .update(cx, |map, cx| map.row_for_block(block_id, cx))
    }

    pub(crate) fn set_focused_block(&mut self, focused_block: FocusedBlock) {
        self.focused_block = Some(focused_block);
    }

    pub(crate) fn take_focused_block(&mut self) -> Option<FocusedBlock> {
        self.focused_block.take()
    }

    pub fn insert_creases(
        &mut self,
        creases: impl IntoIterator<Item = Crease<Anchor>>,
        cx: &mut Context<Self>,
    ) -> Vec<CreaseId> {
        self.display_map
            .update(cx, |map, cx| map.insert_creases(creases, cx))
    }

    pub fn remove_creases(
        &mut self,
        ids: impl IntoIterator<Item = CreaseId>,
        cx: &mut Context<Self>,
    ) -> Vec<(CreaseId, Range<Anchor>)> {
        self.display_map
            .update(cx, |map, cx| map.remove_creases(ids, cx))
    }

    pub fn highlight_rows<T: 'static>(
        &mut self,
        range: Range<Anchor>,
        color: Hsla,
        options: RowHighlightOptions,
        cx: &mut Context<Self>,
    ) {
        let snapshot = self.buffer().read(cx).snapshot(cx);
        let row_highlights = self.highlighted_rows.entry(TypeId::of::<T>()).or_default();
        let index = post_inc(&mut self.highlight_order);
        let ix = row_highlights.binary_search_by(|highlight| {
            Ordering::Equal
                .then_with(|| highlight.range.start.cmp(&range.start, &snapshot))
                .then_with(|| highlight.range.end.cmp(&range.end, &snapshot))
        });

        if let Err(ix) = ix {
            row_highlights.insert(
                ix,
                RowHighlight {
                    range,
                    index,
                    color,
                    options,
                    type_id: TypeId::of::<T>(),
                },
            );
        }
    }

    pub fn remove_highlighted_rows<T: 'static>(
        &mut self,
        ranges_to_remove: Vec<Range<Anchor>>,
        cx: &mut Context<Self>,
    ) {
        let snapshot = self.buffer().read(cx).snapshot(cx);
        let row_highlights = self.highlighted_rows.entry(TypeId::of::<T>()).or_default();
        let mut ranges_to_remove = ranges_to_remove.iter().peekable();
        row_highlights.retain(|highlight| {
            while let Some(range_to_remove) = ranges_to_remove.peek() {
                match range_to_remove.end.cmp(&highlight.range.start, &snapshot) {
                    Ordering::Less | Ordering::Equal => {
                        ranges_to_remove.next();
                    }
                    Ordering::Greater => {
                        match range_to_remove.start.cmp(&highlight.range.end, &snapshot) {
                            Ordering::Less | Ordering::Equal => return false,
                            Ordering::Greater => break,
                        }
                    }
                }
            }
            true
        });
    }

    pub fn clear_row_highlights<T: 'static>(&mut self) {
        self.highlighted_rows.remove(&TypeId::of::<T>());
    }

    pub fn highlighted_rows<T: 'static>(&self) -> impl '_ + Iterator<Item = (Range<Anchor>, Hsla)> {
        self.highlighted_rows
            .get(&TypeId::of::<T>())
            .map_or(&[] as &[_], Vec::as_slice)
            .iter()
            .map(|highlight| (highlight.range.clone(), highlight.color))
    }

    pub fn highlighted_display_rows(
        &self,
        window: &mut Window,
        cx: &mut App,
    ) -> BTreeMap<DisplayRow, LineHighlight> {
        let snapshot = self.snapshot(window, cx);
        let mut used_highlight_orders = HashMap::default();
        self.highlighted_rows
            .values()
            .flat_map(|highlighted_rows| highlighted_rows.iter())
            .fold(
                BTreeMap::<DisplayRow, LineHighlight>::new(),
                |mut unique_rows, highlight| {
                    let start = highlight.range.start.to_display_point(&snapshot);
                    let end = highlight.range.end.to_display_point(&snapshot);
                    let start_row = start.row().0;
                    let end_row = if !highlight.range.end.is_max() && end.column() == 0 {
                        end.row().0.saturating_sub(1)
                    } else {
                        end.row().0
                    };
                    for row in start_row..=end_row {
                        let used_index =
                            used_highlight_orders.entry(row).or_insert(highlight.index);
                        if highlight.index >= *used_index {
                            *used_index = highlight.index;
                            unique_rows.insert(
                                DisplayRow(row),
                                LineHighlight {
                                    include_gutter: highlight.options.include_gutter,
                                    border: None,
                                    background: highlight.color.into(),
                                    type_id: Some(highlight.type_id),
                                },
                            );
                        }
                    }
                    unique_rows
                },
            )
    }
}

impl Editor {
    pub fn move_left(&mut self, _: &MoveLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_with(&mut |map, selection| {
                let cursor = if selection.is_empty() {
                    movement::left(map, selection.start)
                } else {
                    selection.start
                };
                selection.collapse_to(cursor, SelectionGoal::None);
            });
        });
    }

    pub fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, _| {
                (movement::left(map, head), SelectionGoal::None)
            });
        });
    }

    pub fn move_right(&mut self, _: &MoveRight, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_with(&mut |map, selection| {
                let cursor = if selection.is_empty() {
                    movement::right(map, selection.end)
                } else {
                    selection.end
                };
                selection.collapse_to(cursor, SelectionGoal::None);
            });
        });
    }

    pub fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, _| {
                (movement::right(map, head), SelectionGoal::None)
            });
        });
    }

    pub fn move_up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::up(
                    map,
                    selection.start,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });
    }

    pub fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, goal| {
                movement::up(map, head, goal, false, text_layout_details)
            });
        });
    }

    pub fn move_down(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::down(
                    map,
                    selection.end,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });
    }

    pub fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, goal| {
                movement::down(map, head, goal, false, text_layout_details)
            });
        });
    }

    pub fn move_up_by_lines(
        &mut self,
        action: &MoveUpByLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::up_by_rows(
                    map,
                    selection.start,
                    action.lines,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });
    }

    pub fn move_down_by_lines(
        &mut self,
        action: &MoveDownByLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::down_by_rows(
                    map,
                    selection.end,
                    action.lines,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });
    }

    pub fn select_up_by_lines(
        &mut self,
        action: &SelectUpByLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, goal| {
                movement::up_by_rows(map, head, action.lines, goal, false, text_layout_details)
            });
        });
    }

    pub fn select_down_by_lines(
        &mut self,
        action: &SelectDownByLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, goal| {
                movement::down_by_rows(map, head, action.lines, goal, false, text_layout_details)
            });
        });
    }

    pub fn select_page_up(
        &mut self,
        _: &SelectPageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row_count) = self.visible_row_count() else {
            return;
        };
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, goal| {
                movement::up_by_rows(map, head, row_count, goal, false, text_layout_details)
            });
        });
    }

    pub fn move_page_up(
        &mut self,
        action: &MovePageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        let Some(row_count) = self.visible_row_count() else {
            return;
        };
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let effects = if action.center_cursor {
            SelectionEffects::scroll(Autoscroll::center())
        } else {
            SelectionEffects::default()
        };
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(effects, window, cx, |selections| {
            selections.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::up_by_rows(
                    map,
                    selection.end,
                    row_count,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });
    }

    pub fn select_page_down(
        &mut self,
        _: &SelectPageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row_count) = self.visible_row_count() else {
            return;
        };
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, goal| {
                movement::down_by_rows(map, head, row_count, goal, false, text_layout_details)
            });
        });
    }

    pub fn move_page_down(
        &mut self,
        action: &MovePageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        let Some(row_count) = self.visible_row_count() else {
            return;
        };
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let effects = if action.center_cursor {
            SelectionEffects::scroll(Autoscroll::center())
        } else {
            SelectionEffects::default()
        };
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(effects, window, cx, |selections| {
            selections.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::down_by_rows(
                    map,
                    selection.end,
                    row_count,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });
    }

    pub fn move_to_previous_word_start(
        &mut self,
        _: &MoveToPreviousWordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_cursors(window, cx, movement::previous_word_start);
    }

    pub fn move_to_previous_subword_start(
        &mut self,
        _: &MoveToPreviousSubwordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_cursors(window, cx, movement::previous_subword_start);
    }

    pub fn move_to_next_word_end(
        &mut self,
        _: &MoveToNextWordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_cursors(window, cx, movement::next_word_end);
    }

    pub fn move_to_next_subword_end(
        &mut self,
        _: &MoveToNextSubwordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_cursors(window, cx, movement::next_subword_end);
    }

    fn move_cursors(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        movement: fn(&DisplaySnapshot, DisplayPoint) -> DisplayPoint,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections
                .move_cursors_with(&mut |map, head, _| (movement(map, head), SelectionGoal::None));
        });
    }

    pub fn select_to_previous_word_start(
        &mut self,
        _: &SelectToPreviousWordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(window, cx, movement::previous_word_start);
    }

    pub fn select_to_previous_subword_start(
        &mut self,
        _: &SelectToPreviousSubwordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(window, cx, movement::previous_subword_start);
    }

    pub fn select_to_next_word_end(
        &mut self,
        _: &SelectToNextWordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(window, cx, movement::next_word_end);
    }

    pub fn select_to_next_subword_end(
        &mut self,
        _: &SelectToNextSubwordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(window, cx, movement::next_subword_end);
    }

    fn select_to(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        movement: fn(&DisplaySnapshot, DisplayPoint) -> DisplayPoint,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections
                .move_heads_with(&mut |map, head, _| (movement(map, head), SelectionGoal::None));
        });
    }

    pub fn delete_to_previous_word_start(
        &mut self,
        action: &DeleteToPreviousWordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_to_boundary(
            window,
            cx,
            |map, head| {
                if action.ignore_newlines {
                    movement::previous_word_start(map, head)
                } else {
                    movement::previous_word_start_or_newline(map, head)
                }
            },
            action.ignore_brackets,
        );
    }

    pub fn delete_to_previous_subword_start(
        &mut self,
        action: &DeleteToPreviousSubwordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_to_boundary(
            window,
            cx,
            |map, head| {
                if action.ignore_newlines {
                    movement::previous_subword_start(map, head)
                } else {
                    movement::previous_subword_start_or_newline(map, head)
                }
            },
            action.ignore_brackets,
        );
    }

    pub fn delete_to_next_word_end(
        &mut self,
        action: &DeleteToNextWordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_to_boundary(
            window,
            cx,
            |map, head| {
                if action.ignore_newlines {
                    movement::next_word_end(map, head)
                } else {
                    movement::next_word_end_or_newline(map, head)
                }
            },
            action.ignore_brackets,
        );
    }

    pub fn delete_to_next_subword_end(
        &mut self,
        action: &DeleteToNextSubwordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_to_boundary(
            window,
            cx,
            |map, head| {
                if action.ignore_newlines {
                    movement::next_subword_end(map, head)
                } else {
                    movement::next_subword_end_or_newline(map, head)
                }
            },
            action.ignore_brackets,
        );
    }

    fn delete_to_boundary(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        movement: impl Fn(&DisplaySnapshot, DisplayPoint) -> DisplayPoint,
        ignore_brackets: bool,
    ) {
        if self.read_only(cx) {
            return;
        }
        self.hide_mouse_cursor(HideMouseCursorOrigin::TypingAction, cx);
        self.transact(window, cx, |this, window, cx| {
            this.select_autoclose_pair(window, cx);
            this.change_selections(Default::default(), window, cx, |selections| {
                selections.move_with(&mut |map, selection| {
                    if selection.is_empty() {
                        let cursor = movement::adjust_greedy_deletion(
                            map,
                            selection.head(),
                            movement(map, selection.head()),
                            ignore_brackets,
                        );
                        selection.set_head(cursor, SelectionGoal::None);
                    }
                });
            });
            this.insert("", window, cx);
        });
    }

    pub fn move_to_beginning_of_line(
        &mut self,
        action: &MoveToBeginningOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_cursors_with(&mut |map, head, _| {
                (
                    movement::indented_line_beginning(
                        map,
                        head,
                        action.stop_at_soft_wraps,
                        action.stop_at_indent,
                    ),
                    SelectionGoal::None,
                )
            });
        });
    }

    pub fn select_to_beginning_of_line(
        &mut self,
        action: &SelectToBeginningOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, _| {
                (
                    movement::indented_line_beginning(
                        map,
                        head,
                        action.stop_at_soft_wraps,
                        action.stop_at_indent,
                    ),
                    SelectionGoal::None,
                )
            });
        });
    }

    pub fn delete_to_beginning_of_line(
        &mut self,
        action: &DeleteToBeginningOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only(cx) {
            return;
        }
        self.transact(window, cx, |this, window, cx| {
            this.change_selections(Default::default(), window, cx, |selections| {
                selections.move_with(&mut |map, selection| {
                    if selection.is_empty() {
                        let cursor = movement::indented_line_beginning(
                            map,
                            selection.head(),
                            false,
                            action.stop_at_indent,
                        );
                        selection.set_head(cursor, SelectionGoal::None);
                    }
                });
            });
            this.insert("", window, cx);
        });
    }

    pub fn move_to_end_of_line(
        &mut self,
        action: &MoveToEndOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_cursors_with(&mut |map, head, _| {
                (
                    movement::line_end(map, head, action.stop_at_soft_wraps),
                    SelectionGoal::None,
                )
            });
        });
    }

    pub fn select_to_end_of_line(
        &mut self,
        action: &SelectToEndOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, _| {
                (
                    movement::line_end(map, head, action.stop_at_soft_wraps),
                    SelectionGoal::None,
                )
            });
        });
    }

    pub fn delete_to_end_of_line(
        &mut self,
        _: &DeleteToEndOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only(cx) {
            return;
        }
        self.transact(window, cx, |this, window, cx| {
            this.change_selections(Default::default(), window, cx, |selections| {
                selections.move_with(&mut |map, selection| {
                    if selection.is_empty() {
                        let cursor = movement::line_end(map, selection.head(), false);
                        selection.set_head(cursor, SelectionGoal::None);
                    }
                });
            });
            this.insert("", window, cx);
        });
    }

    pub fn cut_to_end_of_line(
        &mut self,
        action: &CutToEndOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_to_end_of_line(&DeleteToEndOfLine, window, cx);
        if !action.stop_at_newlines {
            self.delete(&Delete, window, cx);
        }
    }

    pub fn move_to_start_of_paragraph(
        &mut self,
        _: &MoveToStartOfParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_cursors_with(&mut |map, head, _| {
                (
                    movement::start_of_paragraph(map, head, 1),
                    SelectionGoal::None,
                )
            });
        });
    }

    pub fn move_to_end_of_paragraph(
        &mut self,
        _: &MoveToEndOfParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_cursors_with(&mut |map, head, _| {
                (
                    movement::end_of_paragraph(map, head, 1),
                    SelectionGoal::None,
                )
            });
        });
    }

    pub fn select_to_start_of_paragraph(
        &mut self,
        _: &SelectToStartOfParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, _| {
                (
                    movement::start_of_paragraph(map, head, 1),
                    SelectionGoal::None,
                )
            });
        });
    }

    pub fn select_to_end_of_paragraph(
        &mut self,
        _: &SelectToEndOfParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, _| {
                (
                    movement::end_of_paragraph(map, head, 1),
                    SelectionGoal::None,
                )
            });
        });
    }

    pub fn move_to_start_of_excerpt(
        &mut self,
        _: &MoveToStartOfExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to_excerpt_boundary(
            window,
            cx,
            movement::start_of_excerpt,
            movement::Direction::Prev,
        );
    }

    pub fn move_to_start_of_next_excerpt(
        &mut self,
        _: &MoveToStartOfNextExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to_excerpt_boundary(
            window,
            cx,
            movement::start_of_excerpt,
            movement::Direction::Next,
        );
    }

    pub fn move_to_end_of_excerpt(
        &mut self,
        _: &MoveToEndOfExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to_excerpt_boundary(
            window,
            cx,
            movement::end_of_excerpt,
            movement::Direction::Next,
        );
    }

    pub fn move_to_end_of_previous_excerpt(
        &mut self,
        _: &MoveToEndOfPreviousExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to_excerpt_boundary(
            window,
            cx,
            movement::end_of_excerpt,
            movement::Direction::Prev,
        );
    }

    fn move_to_excerpt_boundary(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        movement: fn(&DisplaySnapshot, DisplayPoint, movement::Direction) -> DisplayPoint,
        direction: movement::Direction,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_cursors_with(&mut |map, head, _| {
                (movement(map, head, direction), SelectionGoal::None)
            });
        });
    }

    pub fn select_to_start_of_excerpt(
        &mut self,
        _: &SelectToStartOfExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to_excerpt_boundary(
            window,
            cx,
            movement::start_of_excerpt,
            movement::Direction::Prev,
        );
    }

    pub fn select_to_start_of_next_excerpt(
        &mut self,
        _: &SelectToStartOfNextExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to_excerpt_boundary(
            window,
            cx,
            movement::start_of_excerpt,
            movement::Direction::Next,
        );
    }

    pub fn select_to_end_of_excerpt(
        &mut self,
        _: &SelectToEndOfExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to_excerpt_boundary(
            window,
            cx,
            movement::end_of_excerpt,
            movement::Direction::Next,
        );
    }

    pub fn select_to_end_of_previous_excerpt(
        &mut self,
        _: &SelectToEndOfPreviousExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to_excerpt_boundary(
            window,
            cx,
            movement::end_of_excerpt,
            movement::Direction::Prev,
        );
    }

    fn select_to_excerpt_boundary(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        movement: fn(&DisplaySnapshot, DisplayPoint, movement::Direction) -> DisplayPoint,
        direction: movement::Direction,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, head, _| {
                (movement(map, head, direction), SelectionGoal::None)
            });
        });
    }

    pub fn move_to_beginning(
        &mut self,
        _: &MoveToBeginning,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections
                .move_cursors_with(&mut |_, _, _| (DisplayPoint::zero(), SelectionGoal::None));
        });
    }

    pub fn select_to_beginning(
        &mut self,
        _: &SelectToBeginning,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |_, _, _| (DisplayPoint::zero(), SelectionGoal::None));
        });
    }

    pub fn move_to_end(&mut self, _: &MoveToEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_cursors_with(&mut |map, _, _| (map.max_point(), SelectionGoal::None));
        });
    }

    pub fn select_to_end(&mut self, _: &SelectToEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_heads_with(&mut |map, _, _| (map.max_point(), SelectionGoal::None));
        });
    }

    pub fn set_nav_history(&mut self, nav_history: Option<ItemNavHistory>) {
        self.nav_history = nav_history;
    }

    pub fn nav_history(&self) -> Option<&ItemNavHistory> {
        self.nav_history.as_ref()
    }

    pub fn save_location(
        &mut self,
        _: &SaveLocation,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchor = self.selections.newest_anchor().head();
        self.push_to_nav_history(anchor, None, false, false, cx);
    }

    fn navigation_data(&self, cursor_anchor: Anchor, cx: &mut Context<Self>) -> NavigationData {
        let snapshot = self.display_snapshot(cx);
        let scroll_anchor = self.scroll_manager.native_anchor(&snapshot, cx);
        let scroll_top_row = scroll_anchor.anchor.to_display_point(&snapshot).row().0;
        NavigationData {
            cursor_anchor,
            cursor_position: cursor_anchor.to_point(snapshot.buffer_snapshot()),
            scroll_anchor,
            scroll_top_row,
        }
    }

    fn push_to_nav_history(
        &mut self,
        anchor: Anchor,
        cursor_position: Option<Point>,
        is_deactivate: bool,
        only_if_new: bool,
        cx: &mut Context<Self>,
    ) {
        let mut entry = self.navigation_data(anchor, cx);
        if let Some(cursor_position) = cursor_position {
            entry.cursor_position = cursor_position;
        }
        if let Some(nav_history) = &mut self.nav_history {
            let _ = only_if_new;
            nav_history.push(Some(entry), Some(entry.scroll_top_row), cx);
            cx.emit(EditorEvent::PushedToNavHistory {
                anchor,
                is_deactivate,
            });
        }
    }

    pub fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let snapshot = self.buffer.read(cx).read(cx);
        let start = snapshot.anchor_before(MultiBufferOffset(0));
        let end = snapshot.anchor_after(snapshot.len());
        drop(snapshot);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.select_anchor_ranges([start..end]);
        });
    }

    pub fn select_line(&mut self, _: &SelectLine, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.move_with(&mut |map, selection| {
                let point = selection.head().to_point(map);
                let row = MultiBufferRow(point.row);
                let start = Point::new(point.row, 0);
                let end = if row < map.buffer_snapshot().max_row() {
                    Point::new(point.row + 1, 0)
                } else {
                    Point::new(point.row, map.buffer_snapshot().line_len(row))
                };
                selection.start = start.to_display_point(map);
                selection.end = end.to_display_point(map);
                selection.reversed = false;
                selection.goal = SelectionGoal::None;
            });
        });
    }

    pub fn split_selection_into_lines(
        &mut self,
        action: &SplitSelectionIntoLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let display_map = self.display_snapshot(cx);
        let mut ranges = Vec::new();
        for selection in self.selections.all::<Point>(&display_map) {
            for row in selection.spanned_rows(false, &display_map).iter_rows() {
                let line_start = Point::new(row.0, 0);
                let line_end = Point::new(row.0, display_map.buffer_snapshot().line_len(row));
                if action.keep_selections {
                    ranges.push(line_start..line_end);
                } else {
                    ranges.push(line_start..line_start);
                }
            }
        }
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges(ranges);
        });
    }

    pub fn add_selection_above(
        &mut self,
        action: &AddSelectionAbove,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_selection(action.skip_soft_wrap, true, window, cx);
    }

    pub fn add_selection_below(
        &mut self,
        action: &AddSelectionBelow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_selection(action.skip_soft_wrap, false, window, cx);
    }

    fn add_selection(
        &mut self,
        skip_soft_wrap: bool,
        above: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_mouse_cursor(HideMouseCursorOrigin::MovementAction, cx);
        let text_layout_details = &self.text_layout_details(window, cx);
        let display_map = self.display_snapshot(cx);
        let newest = self.selections.newest::<Point>(&display_map);
        let newest_head = newest.head().to_display_point(&display_map);
        let (head, goal) = if above {
            movement::up(
                &display_map,
                newest_head,
                newest.goal,
                skip_soft_wrap,
                text_layout_details,
            )
        } else {
            movement::down(
                &display_map,
                newest_head,
                newest.goal,
                skip_soft_wrap,
                text_layout_details,
            )
        };
        let head_point = head.to_point(&display_map);
        self.change_selections(Default::default(), window, cx, |selections| {
            selections.insert_range(head_point..head_point);
            selections.move_with(&mut |_, selection| {
                if selection.head() == head {
                    selection.goal = goal;
                }
            });
        });
    }
}

fn comment_delimiter_for_newline(
    start_point: &Point,
    buffer: &MultiBufferSnapshot,
    language: &LanguageScope,
) -> Option<Arc<str>> {
    let delimiters = language.line_comment_prefixes();
    let max_len_of_delimiter = delimiters.iter().map(|delimiter| delimiter.len()).max()?;
    let (snapshot, range) = buffer.buffer_line_for_row(MultiBufferRow(start_point.row))?;

    let num_of_whitespaces = snapshot
        .chars_for_range(range.clone())
        .take_while(|c| c.is_whitespace())
        .count();
    let comment_candidate = snapshot
        .chars_for_range(range.clone())
        .skip(num_of_whitespaces)
        .take(max_len_of_delimiter + 2)
        .collect::<String>();
    let (delimiter, trimmed_len, is_repl) = delimiters
        .iter()
        .filter_map(|delimiter| {
            let prefix = delimiter.trim_end();
            if comment_candidate.starts_with(prefix) {
                let is_repl = if let Some(stripped_comment) = comment_candidate.strip_prefix(prefix)
                {
                    stripped_comment.starts_with(" %%")
                } else {
                    false
                };
                Some((delimiter, prefix.len(), is_repl))
            } else {
                None
            }
        })
        .max_by_key(|(_, len, _)| *len)?;

    if let Some(BlockCommentConfig {
        start: block_start, ..
    }) = language.block_comment()
    {
        let block_start_trimmed = block_start.trim_end();
        if block_start_trimmed.starts_with(delimiter.trim_end()) {
            let line_content = snapshot
                .chars_for_range(range.clone())
                .skip(num_of_whitespaces)
                .take(block_start_trimmed.len())
                .collect::<String>();

            if line_content.starts_with(block_start_trimmed) {
                return None;
            }
        }
    }

    let cursor_is_placed_after_comment_marker =
        num_of_whitespaces + trimmed_len <= start_point.column as usize;
    if cursor_is_placed_after_comment_marker {
        if !is_repl {
            return Some(delimiter.clone());
        }

        let line_content_after_cursor: String = snapshot
            .chars_for_range(range)
            .skip(start_point.column as usize)
            .collect();

        if line_content_after_cursor.trim().is_empty() {
            return None;
        } else {
            return Some(delimiter.clone());
        }
    } else {
        None
    }
}

fn documentation_delimiter_for_newline(
    start_point: &Point,
    buffer: &MultiBufferSnapshot,
    language: &LanguageScope,
    newline_config: &mut NewlineConfig,
) -> Option<Arc<str>> {
    let BlockCommentConfig {
        start: start_tag,
        end: end_tag,
        prefix: delimiter,
        tab_size: len,
    } = language.documentation_comment()?;
    let is_within_block_comment = buffer
        .language_scope_at(*start_point)
        .is_some_and(|scope| scope.override_name() == Some("comment"));
    if !is_within_block_comment {
        return None;
    }

    let (snapshot, range) = buffer.buffer_line_for_row(MultiBufferRow(start_point.row))?;

    let num_of_whitespaces = snapshot
        .chars_for_range(range.clone())
        .take_while(|c| c.is_whitespace())
        .count();

    // It is safe to use a column from MultiBufferPoint in context of a single buffer ranges, because we're only ever looking at a single line at a time.
    let column = start_point.column;
    let cursor_is_after_start_tag = {
        let start_tag_len = start_tag.len();
        let start_tag_line = snapshot
            .chars_for_range(range.clone())
            .skip(num_of_whitespaces)
            .take(start_tag_len)
            .collect::<String>();
        if start_tag_line.starts_with(start_tag.as_ref()) {
            num_of_whitespaces + start_tag_len <= column as usize
        } else {
            false
        }
    };

    let cursor_is_after_delimiter = {
        let delimiter_trim = delimiter.trim_end();
        let delimiter_line = snapshot
            .chars_for_range(range.clone())
            .skip(num_of_whitespaces)
            .take(delimiter_trim.len())
            .collect::<String>();
        if delimiter_line.starts_with(delimiter_trim) {
            num_of_whitespaces + delimiter_trim.len() <= column as usize
        } else {
            false
        }
    };

    let mut needs_extra_line = false;
    let mut extra_line_additional_indent = IndentSize::spaces(0);

    let cursor_is_before_end_tag_if_exists = {
        let mut char_position = 0u32;
        let mut end_tag_offset = None;

        'outer: for chunk in snapshot.text_for_range(range) {
            if let Some(byte_pos) = chunk.find(&**end_tag) {
                let chars_before_match = chunk[..byte_pos].chars().count() as u32;
                end_tag_offset = Some(char_position + chars_before_match);
                break 'outer;
            }
            char_position += chunk.chars().count() as u32;
        }

        if let Some(end_tag_offset) = end_tag_offset {
            let cursor_is_before_end_tag = column <= end_tag_offset;
            if cursor_is_after_start_tag {
                if cursor_is_before_end_tag {
                    needs_extra_line = true;
                }
                let cursor_is_at_start_of_end_tag = column == end_tag_offset;
                if cursor_is_at_start_of_end_tag {
                    extra_line_additional_indent.len = *len;
                }
            }
            cursor_is_before_end_tag
        } else {
            true
        }
    };

    if (cursor_is_after_start_tag || cursor_is_after_delimiter)
        && cursor_is_before_end_tag_if_exists
    {
        let additional_indent = if cursor_is_after_start_tag {
            IndentSize::spaces(*len)
        } else {
            IndentSize::spaces(0)
        };

        *newline_config = NewlineConfig::Newline {
            additional_indent,
            extra_line_additional_indent: if needs_extra_line {
                Some(extra_line_additional_indent)
            } else {
                None
            },
            prevent_auto_indent: true,
        };
        Some(delimiter.clone())
    } else {
        None
    }
}

const ORDERED_LIST_MAX_MARKER_LEN: usize = 16;

fn list_delimiter_for_newline(
    start_point: &Point,
    buffer: &MultiBufferSnapshot,
    language: &LanguageScope,
    newline_config: &mut NewlineConfig,
) -> Option<Arc<str>> {
    let (snapshot, range) = buffer.buffer_line_for_row(MultiBufferRow(start_point.row))?;

    let num_of_whitespaces = snapshot
        .chars_for_range(range.clone())
        .take_while(|c| c.is_whitespace())
        .count();

    let task_list_entries: Vec<_> = language
        .task_list()
        .into_iter()
        .flat_map(|config| {
            config
                .prefixes
                .iter()
                .map(|prefix| (prefix.as_ref(), config.continuation.as_ref()))
        })
        .collect();
    let unordered_list_entries: Vec<_> = language
        .unordered_list()
        .iter()
        .map(|marker| (marker.as_ref(), marker.as_ref()))
        .collect();

    let all_entries: Vec<_> = task_list_entries
        .into_iter()
        .chain(unordered_list_entries)
        .collect();

    if let Some(max_prefix_len) = all_entries.iter().map(|(p, _)| p.len()).max() {
        let candidate: String = snapshot
            .chars_for_range(range.clone())
            .skip(num_of_whitespaces)
            .take(max_prefix_len)
            .collect();

        if let Some((prefix, continuation)) = all_entries
            .iter()
            .filter(|(prefix, _)| candidate.starts_with(*prefix))
            .max_by_key(|(prefix, _)| prefix.len())
        {
            let end_of_prefix = num_of_whitespaces + prefix.len();
            let cursor_is_after_prefix = end_of_prefix <= start_point.column as usize;
            let has_content_after_marker = snapshot
                .chars_for_range(range)
                .skip(end_of_prefix)
                .any(|c| !c.is_whitespace());

            if has_content_after_marker && cursor_is_after_prefix {
                return Some((*continuation).into());
            }

            if start_point.column as usize == end_of_prefix {
                if num_of_whitespaces == 0 {
                    *newline_config = NewlineConfig::ClearCurrentLine;
                } else {
                    *newline_config = NewlineConfig::UnindentCurrentLine {
                        continuation: (*continuation).into(),
                    };
                }
            }

            return None;
        }
    }

    let candidate: String = snapshot
        .chars_for_range(range.clone())
        .skip(num_of_whitespaces)
        .take(ORDERED_LIST_MAX_MARKER_LEN)
        .collect();

    for ordered_config in language.ordered_list() {
        let regex = match Regex::new(&ordered_config.pattern) {
            Ok(r) => r,
            Err(_) => continue,
        };

        if let Some(captures) = regex.captures(&candidate) {
            let full_match = captures.get(0)?;
            let marker_len = full_match.len();
            let end_of_prefix = num_of_whitespaces + marker_len;
            let cursor_is_after_prefix = end_of_prefix <= start_point.column as usize;

            let has_content_after_marker = snapshot
                .chars_for_range(range)
                .skip(end_of_prefix)
                .any(|c| !c.is_whitespace());

            if has_content_after_marker && cursor_is_after_prefix {
                let number: u32 = captures.get(1)?.as_str().parse().ok()?;
                let continuation = ordered_config
                    .format
                    .replace("{1}", &(number + 1).to_string());
                return Some(continuation.into());
            }

            if start_point.column as usize == end_of_prefix {
                let continuation = ordered_config.format.replace("{1}", "1");
                if num_of_whitespaces == 0 {
                    *newline_config = NewlineConfig::ClearCurrentLine;
                } else {
                    *newline_config = NewlineConfig::UnindentCurrentLine {
                        continuation: continuation.into(),
                    };
                }
            }

            return None;
        }
    }

    None
}

fn is_list_prefix_row(
    row: MultiBufferRow,
    buffer: &MultiBufferSnapshot,
    language: &LanguageScope,
) -> bool {
    let Some((snapshot, range)) = buffer.buffer_line_for_row(row) else {
        return false;
    };

    let num_of_whitespaces = snapshot
        .chars_for_range(range.clone())
        .take_while(|c| c.is_whitespace())
        .count();

    let task_list_prefixes: Vec<_> = language
        .task_list()
        .into_iter()
        .flat_map(|config| {
            config
                .prefixes
                .iter()
                .map(|p| p.as_ref())
                .collect::<Vec<_>>()
        })
        .collect();
    let unordered_list_markers: Vec<_> = language
        .unordered_list()
        .iter()
        .map(|marker| marker.as_ref())
        .collect();
    let all_prefixes: Vec<_> = task_list_prefixes
        .into_iter()
        .chain(unordered_list_markers)
        .collect();
    if let Some(max_prefix_len) = all_prefixes.iter().map(|p| p.len()).max() {
        let candidate: String = snapshot
            .chars_for_range(range.clone())
            .skip(num_of_whitespaces)
            .take(max_prefix_len)
            .collect();
        if all_prefixes
            .iter()
            .any(|prefix| candidate.starts_with(*prefix))
        {
            return true;
        }
    }

    let ordered_list_candidate: String = snapshot
        .chars_for_range(range)
        .skip(num_of_whitespaces)
        .take(ORDERED_LIST_MAX_MARKER_LEN)
        .collect();
    for ordered_config in language.ordered_list() {
        let regex = match Regex::new(&ordered_config.pattern) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if let Some(captures) = regex.captures(&ordered_list_candidate) {
            return captures.get(0).is_some();
        }
    }

    false
}

#[derive(Debug)]
enum NewlineConfig {
    /// Insert newline with optional additional indent and optional extra blank line
    Newline {
        additional_indent: IndentSize,
        extra_line_additional_indent: Option<IndentSize>,
        prevent_auto_indent: bool,
    },
    /// Clear the current line
    ClearCurrentLine,
    /// Unindent the current line and add continuation
    UnindentCurrentLine { continuation: Arc<str> },
}

impl NewlineConfig {
    fn has_extra_line(&self) -> bool {
        matches!(
            self,
            Self::Newline {
                extra_line_additional_indent: Some(_),
                ..
            }
        )
    }

    fn insert_extra_newline_brackets(
        buffer: &MultiBufferSnapshot,
        range: Range<MultiBufferOffset>,
        language: &language::LanguageScope,
    ) -> bool {
        let leading_whitespace_len = buffer
            .reversed_chars_at(range.start)
            .take_while(|c| c.is_whitespace() && *c != '\n')
            .map(|c| c.len_utf8())
            .sum::<usize>();
        let trailing_whitespace_len = buffer
            .chars_at(range.end)
            .take_while(|c| c.is_whitespace() && *c != '\n')
            .map(|c| c.len_utf8())
            .sum::<usize>();
        let range = range.start - leading_whitespace_len..range.end + trailing_whitespace_len;

        language.brackets().any(|(pair, enabled)| {
            let pair_start = pair.start.trim_end();
            let pair_end = pair.end.trim_start();

            enabled
                && pair.newline
                && buffer.contains_str_at(range.end, pair_end)
                && buffer.contains_str_at(
                    range.start.saturating_sub_usize(pair_start.len()),
                    pair_start,
                )
        })
    }

    fn insert_extra_newline_tree_sitter(
        buffer: &MultiBufferSnapshot,
        range: Range<MultiBufferOffset>,
    ) -> bool {
        let (buffer, range) = match buffer
            .range_to_buffer_ranges(range.start..range.end)
            .as_slice()
        {
            [(buffer_snapshot, range, _)] => (buffer_snapshot.clone(), range.clone()),
            _ => return false,
        };
        let pair = {
            let mut result: Option<BracketMatch<usize>> = None;

            for pair in buffer
                .all_bracket_ranges(range.start.0..range.end.0)
                .filter(move |pair| {
                    pair.open_range.start <= range.start.0 && pair.close_range.end >= range.end.0
                })
            {
                let len = pair.close_range.end - pair.open_range.start;

                if let Some(existing) = &result {
                    let existing_len = existing.close_range.end - existing.open_range.start;
                    if len > existing_len {
                        continue;
                    }
                }

                result = Some(pair);
            }

            result
        };
        let Some(pair) = pair else {
            return false;
        };
        pair.newline_only
            && buffer
                .chars_for_range(pair.open_range.end..range.start.0)
                .chain(buffer.chars_for_range(range.end.0..pair.close_range.start))
                .all(|c| c.is_whitespace() && c != '\n')
    }
}

impl EditorSnapshot {
    pub fn language_at<T: ToOffset>(&self, position: T) -> Option<&Arc<Language>> {
        self.display_snapshot
            .buffer_snapshot()
            .language_at(position)
    }

    pub fn is_focused(&self) -> bool {
        self.is_focused
    }

    pub fn placeholder_text(&self) -> Option<String> {
        self.placeholder_display_snapshot
            .as_ref()
            .map(|display_map| display_map.text())
    }

    pub fn scroll_position(&self) -> gpui::Point<ScrollOffset> {
        self.scroll_anchor.scroll_position(&self.display_snapshot)
    }

    pub fn gutter_dimensions(
        &self,
        font_id: FontId,
        font_size: Pixels,
        style: &EditorStyle,
        window: &mut Window,
        cx: &App,
    ) -> GutterDimensions {
        if self.show_gutter
            && let Some(ch_width) = cx.text_system().ch_width(font_id, font_size).log_err()
            && let Some(ch_advance) = cx.text_system().ch_advance(font_id, font_size).log_err()
        {
            let gutter_settings = EditorSettings::get_global(cx).gutter;
            let show_line_numbers = self
                .show_line_numbers
                .unwrap_or(gutter_settings.line_numbers);
            let line_gutter_width = if show_line_numbers {
                // Avoid flicker-like gutter resizes when the line number gains another digit by
                // only resizing the gutter on files with > 10**min_line_number_digits lines.
                let min_width_for_number_on_gutter =
                    ch_advance * gutter_settings.min_line_number_digits as f32;
                self.max_line_number_width(style, window)
                    .max(min_width_for_number_on_gutter)
            } else {
                0.0.into()
            };

            let is_singleton = self.buffer_snapshot().is_singleton();

            let left_padding = if !is_singleton {
                ch_width * 4.0
            } else if show_line_numbers {
                ch_width
            } else {
                px(0.)
            };

            let shows_folds = is_singleton && gutter_settings.folds;

            let right_padding = if shows_folds && show_line_numbers {
                ch_width * 4.0
            } else if shows_folds || (!is_singleton && show_line_numbers) {
                ch_width * 3.0
            } else if show_line_numbers {
                ch_width
            } else {
                px(0.)
            };

            GutterDimensions {
                left_padding,
                right_padding,
                width: line_gutter_width + left_padding + right_padding,
                margin: GutterDimensions::default_gutter_margin(font_id, font_size, cx),
            }
        } else if self.offset_content {
            GutterDimensions::default_with_margin(font_id, font_size, cx)
        } else {
            GutterDimensions::default()
        }
    }

    pub fn render_crease_toggle(
        &self,
        buffer_row: MultiBufferRow,
        row_contains_cursor: bool,
        editor: Entity<Editor>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let folded = self.is_line_folded(buffer_row);
        let mut is_foldable = false;

        if let Some(crease) = self
            .crease_snapshot
            .query_row(buffer_row, self.buffer_snapshot())
        {
            is_foldable = true;
            match crease {
                Crease::Inline { render_toggle, .. } | Crease::Block { render_toggle, .. } => {
                    if let Some(render_toggle) = render_toggle {
                        let toggle_callback =
                            Arc::new(move |folded, window: &mut Window, cx: &mut App| {
                                if folded {
                                    editor.update(cx, |editor, cx| {
                                        editor.fold_at(buffer_row, window, cx)
                                    });
                                } else {
                                    editor.update(cx, |editor, cx| {
                                        editor.unfold_at(buffer_row, window, cx)
                                    });
                                }
                            });
                        return Some((render_toggle)(
                            buffer_row,
                            folded,
                            toggle_callback,
                            window,
                            cx,
                        ));
                    }
                }
            }
        }

        is_foldable |= self.starts_indent(buffer_row);

        if folded || (is_foldable && (row_contains_cursor || self.gutter_hovered)) {
            Some(
                Disclosure::new(("gutter_crease", buffer_row.0), !folded)
                    .toggle_state(folded)
                    .on_click(window.listener_for(&editor, move |this, _e, window, cx| {
                        if folded {
                            this.unfold_at(buffer_row, window, cx);
                        } else {
                            this.fold_at(buffer_row, window, cx);
                        }
                    }))
                    .into_any_element(),
            )
        } else {
            None
        }
    }

    pub fn render_crease_trailer(
        &self,
        buffer_row: MultiBufferRow,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let folded = self.is_line_folded(buffer_row);
        if let Crease::Inline { render_trailer, .. } = self
            .crease_snapshot
            .query_row(buffer_row, self.buffer_snapshot())?
        {
            let render_trailer = render_trailer.as_ref()?;
            Some(render_trailer(buffer_row, folded, window, cx))
        } else {
            None
        }
    }

    pub fn max_line_number_width(&self, style: &EditorStyle, window: &mut Window) -> Pixels {
        let digit_count = self.widest_line_number().ilog10() + 1;
        column_pixels(style, digit_count as usize, window)
    }

    /// Returns the line delta from `base` to `line` in the multibuffer, ignoring wrapped lines.
    ///
    /// This is positive if `base` is before `line`.
    fn relative_line_delta(
        &self,
        current_selection_head: DisplayRow,
        first_visible_row: DisplayRow,
        consider_wrapped_lines: bool,
    ) -> i64 {
        let current_selection_head = current_selection_head.as_display_point().to_point(self);
        let first_visible_row = first_visible_row.as_display_point().to_point(self);

        if consider_wrapped_lines {
            let wrap_snapshot = self.wrap_snapshot();
            let base_wrap_row = wrap_snapshot
                .make_wrap_point(current_selection_head, Bias::Left)
                .row();
            let wrap_row = wrap_snapshot
                .make_wrap_point(first_visible_row, Bias::Left)
                .row();

            wrap_row.0 as i64 - base_wrap_row.0 as i64
        } else {
            let fold_snapshot = self.fold_snapshot();
            let base_fold_row = fold_snapshot
                .to_fold_point(self.to_inlay_point(current_selection_head), Bias::Left)
                .row();
            let fold_row = fold_snapshot
                .to_fold_point(self.to_inlay_point(first_visible_row), Bias::Left)
                .row();

            fold_row as i64 - base_fold_row as i64
        }
    }

    /// Returns the unsigned relative line number to display for each row in `rows`.
    ///
    /// Wrapped rows are excluded from the hashmap if `count_relative_lines` is `false`.
    pub fn calculate_relative_line_numbers(
        &self,
        rows: &Range<DisplayRow>,
        current_selection_head: DisplayRow,
        count_wrapped_lines: bool,
    ) -> HashMap<DisplayRow, u32> {
        let initial_offset =
            self.relative_line_delta(current_selection_head, rows.start, count_wrapped_lines);

        self.row_infos(rows.start)
            .take(rows.len())
            .enumerate()
            .map(|(i, row_info)| (DisplayRow(rows.start.0 + i as u32), row_info))
            .filter(|(_row, row_info)| {
                row_info.buffer_row.is_some()
                    || (count_wrapped_lines && row_info.wrapped_buffer_row.is_some())
            })
            .enumerate()
            .filter_map(|(i, (row, _row_info))| {
                // We want to ensure here that the current line has absolute
                // numbering, even if we are in a soft-wrapped line.
                let relative_line_number = (initial_offset + i as i64).unsigned_abs() as u32;

                (relative_line_number != 0).then_some((row, relative_line_number))
            })
            .collect()
    }
}

pub fn column_pixels(style: &EditorStyle, column: usize, window: &Window) -> Pixels {
    let font_size = style.text.font_size.to_pixels(window.rem_size());
    let layout = window.text_system().shape_line(
        SharedString::from(" ".repeat(column)),
        font_size,
        &[TextRun {
            len: column,
            font: style.text.font(),
            color: Hsla::default(),
            ..Default::default()
        }],
        None,
    );

    layout.width
}

impl Deref for EditorSnapshot {
    type Target = DisplaySnapshot;

    fn deref(&self) -> &Self::Target {
        &self.display_snapshot
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEvent {
    InputIgnored {
        text: Arc<str>,
    },
    InputHandled {
        utf16_range_to_replace: Option<Range<isize>>,
        text: Arc<str>,
    },
    BufferRangesUpdated {
        buffer: Entity<Buffer>,
        path_key: PathKey,
        ranges: Vec<ExcerptRange<text::Anchor>>,
    },
    BuffersRemoved {
        removed_buffer_ids: Vec<BufferId>,
    },
    BuffersEdited {
        buffer_ids: Vec<BufferId>,
    },
    BufferFoldToggled {
        ids: Vec<BufferId>,
        folded: bool,
    },
    ExpandExcerptsRequested {
        excerpt_anchors: Vec<Anchor>,
        lines: u32,
        direction: ExpandExcerptDirection,
    },
    OpenExcerptsRequested {
        selections_by_buffer: HashMap<BufferId, (Vec<Range<BufferOffset>>, Option<u32>)>,
        split: bool,
    },
    BufferEdited,
    Edited {
        transaction_id: clock::Lamport,
    },
    Reparsed(BufferId),
    Focused,
    FocusedIn,
    Blurred,
    DirtyChanged,
    Saved,
    TitleChanged,
    FileHandleChanged,
    SelectionsChanged {
        local: bool,
    },
    ScrollPositionChanged {
        autoscroll: bool,
    },
    TransactionUndone {
        transaction_id: clock::Lamport,
    },
    TransactionBegun {
        transaction_id: clock::Lamport,
    },
    CursorShapeChanged,
    PushedToNavHistory {
        anchor: Anchor,
        is_deactivate: bool,
    },
}

impl EventEmitter<EditorEvent> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        EditorElement::new(&cx.entity(), self.create_style(cx))
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let snapshot = self.buffer.read(cx).read(cx);
        let start = snapshot.clip_offset_utf16(
            MultiBufferOffsetUtf16(OffsetUtf16(range_utf16.start)),
            Bias::Left,
        );
        let end = snapshot.clip_offset_utf16(
            MultiBufferOffsetUtf16(OffsetUtf16(range_utf16.end)),
            Bias::Right,
        );
        if (start.0.0..end.0.0) != range_utf16 {
            adjusted_range.replace(start.0.0..end.0.0);
        }
        Some(snapshot.text_for_range(start..end).collect())
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        // Prevent the IME menu from appearing when holding down an alphabetic key
        // while input is disabled.
        if !ignore_disabled_input && !self.input_enabled {
            return None;
        }

        let selection = self
            .selections
            .newest::<MultiBufferOffsetUtf16>(&self.display_snapshot(cx));
        let range = selection.range();

        Some(UTF16Selection {
            range: range.start.0.0..range.end.0.0,
            reversed: selection.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let snapshot = self.buffer.read(cx).read(cx);
        let range = self
            .text_highlights(HighlightKey::InputComposition, cx)?
            .1
            .first()?;
        Some(range.start.to_offset_utf16(&snapshot).0.0..range.end.to_offset_utf16(&snapshot).0.0)
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_highlights(HighlightKey::InputComposition, cx);
        self.ime_transaction.take();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.input_enabled {
            cx.emit(EditorEvent::InputIgnored { text: text.into() });
            return;
        }

        self.transact(window, cx, |this, window, cx| {
            let new_selected_ranges = if let Some(range_utf16) = range_utf16 {
                if let Some(marked_ranges) = this.marked_text_ranges(cx) {
                    // During IME composition, macOS reports the replacement range
                    // relative to the first marked region (the only one visible via
                    // marked_text_range). The correct targets for replacement are the
                    // marked ranges themselves — one per cursor — so use them directly.
                    Some(marked_ranges)
                } else if range_utf16.start == range_utf16.end {
                    // An empty replacement range means "insert at cursor" with no text
                    // to replace. macOS reports the cursor position from its own
                    // (single-cursor) view of the buffer, which diverges from our actual
                    // cursor positions after multi-cursor edits have shifted offsets.
                    // Treating this as range_utf16=None lets each cursor insert in place.
                    None
                } else {
                    // Outside of IME composition (e.g. Accessibility Keyboard word
                    // completion), the range is an absolute document offset for the
                    // newest cursor. Fan it out to all cursors via
                    // selection_replacement_ranges, which applies the delta relative
                    // to the newest selection to every cursor.
                    let range_utf16 = MultiBufferOffsetUtf16(OffsetUtf16(range_utf16.start))
                        ..MultiBufferOffsetUtf16(OffsetUtf16(range_utf16.end));
                    Some(this.selection_replacement_ranges(range_utf16, cx))
                }
            } else {
                this.marked_text_ranges(cx)
            };

            let range_to_replace = new_selected_ranges.as_ref().and_then(|ranges_to_replace| {
                let newest_selection_id = this.selections.newest_anchor().id;
                this.selections
                    .all::<MultiBufferOffsetUtf16>(&this.display_snapshot(cx))
                    .iter()
                    .zip(ranges_to_replace.iter())
                    .find_map(|(selection, range)| {
                        if selection.id == newest_selection_id {
                            Some(
                                (range.start.0.0 as isize - selection.head().0.0 as isize)
                                    ..(range.end.0.0 as isize - selection.head().0.0 as isize),
                            )
                        } else {
                            None
                        }
                    })
            });

            cx.emit(EditorEvent::InputHandled {
                utf16_range_to_replace: range_to_replace,
                text: text.into(),
            });

            if let Some(new_selected_ranges) = new_selected_ranges {
                // Only backspace if at least one range covers actual text. When all
                // ranges are empty (e.g. a trailing-space insertion from Accessibility
                // Keyboard sends replacementRange=cursor..cursor), backspace would
                // incorrectly delete the character just before the cursor.
                let should_backspace = new_selected_ranges.iter().any(|r| r.start != r.end);
                this.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                    selections.select_ranges(new_selected_ranges)
                });
                if should_backspace {
                    this.backspace(&Default::default(), window, cx);
                }
            }

            this.handle_input(text, window, cx);
        });

        if let Some(transaction) = self.ime_transaction {
            self.buffer.update(cx, |buffer, cx| {
                buffer.group_until_transaction(transaction, cx);
            });
        }

        self.unmark_text(window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.input_enabled {
            return;
        }

        let transaction = self.transact(window, cx, |this, window, cx| {
            let ranges_to_replace = if let Some(mut marked_ranges) = this.marked_text_ranges(cx) {
                let snapshot = this.buffer.read(cx).read(cx);
                if let Some(relative_range_utf16) = range_utf16.as_ref() {
                    for marked_range in &mut marked_ranges {
                        marked_range.end = marked_range.start + relative_range_utf16.end;
                        marked_range.start += relative_range_utf16.start;
                        marked_range.start =
                            snapshot.clip_offset_utf16(marked_range.start, Bias::Left);
                        marked_range.end =
                            snapshot.clip_offset_utf16(marked_range.end, Bias::Right);
                    }
                }
                Some(marked_ranges)
            } else if let Some(range_utf16) = range_utf16 {
                let range_utf16 = MultiBufferOffsetUtf16(OffsetUtf16(range_utf16.start))
                    ..MultiBufferOffsetUtf16(OffsetUtf16(range_utf16.end));
                Some(this.selection_replacement_ranges(range_utf16, cx))
            } else {
                None
            };

            let range_to_replace = ranges_to_replace.as_ref().and_then(|ranges_to_replace| {
                let newest_selection_id = this.selections.newest_anchor().id;
                this.selections
                    .all::<MultiBufferOffsetUtf16>(&this.display_snapshot(cx))
                    .iter()
                    .zip(ranges_to_replace.iter())
                    .find_map(|(selection, range)| {
                        if selection.id == newest_selection_id {
                            Some(
                                (range.start.0.0 as isize - selection.head().0.0 as isize)
                                    ..(range.end.0.0 as isize - selection.head().0.0 as isize),
                            )
                        } else {
                            None
                        }
                    })
            });

            cx.emit(EditorEvent::InputHandled {
                utf16_range_to_replace: range_to_replace,
                text: text.into(),
            });

            if let Some(ranges) = ranges_to_replace {
                this.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                    s.select_ranges(ranges)
                });
            }

            let marked_ranges = {
                let snapshot = this.buffer.read(cx).read(cx);
                this.selections
                    .disjoint_anchors_arc()
                    .iter()
                    .map(|selection| {
                        selection.start.bias_left(&snapshot)..selection.end.bias_right(&snapshot)
                    })
                    .collect::<Vec<_>>()
            };

            if text.is_empty() {
                this.unmark_text(window, cx);
            } else {
                this.highlight_text(
                    HighlightKey::InputComposition,
                    marked_ranges.clone(),
                    HighlightStyle {
                        underline: Some(UnderlineStyle {
                            thickness: px(1.),
                            color: None,
                            wavy: false,
                        }),
                        ..Default::default()
                    },
                    cx,
                );
            }

            // Disable auto-closing when composing text (i.e. typing a `"` on a Brazilian keyboard)
            let use_autoclose = this.use_autoclose;
            let use_auto_surround = this.use_auto_surround;
            this.set_use_autoclose(false);
            this.set_use_auto_surround(false);
            this.handle_input(text, window, cx);
            this.set_use_autoclose(use_autoclose);
            this.set_use_auto_surround(use_auto_surround);

            if let Some(new_selected_range) = new_selected_range_utf16 {
                let snapshot = this.buffer.read(cx).read(cx);
                let new_selected_ranges = marked_ranges
                    .into_iter()
                    .map(|marked_range| {
                        let insertion_start = marked_range.start.to_offset_utf16(&snapshot).0;
                        let new_start = MultiBufferOffsetUtf16(OffsetUtf16(
                            insertion_start.0 + new_selected_range.start,
                        ));
                        let new_end = MultiBufferOffsetUtf16(OffsetUtf16(
                            insertion_start.0 + new_selected_range.end,
                        ));
                        snapshot.clip_offset_utf16(new_start, Bias::Left)
                            ..snapshot.clip_offset_utf16(new_end, Bias::Right)
                    })
                    .collect::<Vec<_>>();

                drop(snapshot);
                this.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                    selections.select_ranges(new_selected_ranges)
                });
            }
        });

        self.ime_transaction = self.ime_transaction.or(transaction);
        if let Some(transaction) = self.ime_transaction {
            self.buffer.update(cx, |buffer, cx| {
                buffer.group_until_transaction(transaction, cx);
            });
        }

        if self
            .text_highlights(HighlightKey::InputComposition, cx)
            .is_none()
        {
            self.ime_transaction.take();
        }
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: gpui::Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Bounds<Pixels>> {
        let text_layout_details = self.text_layout_details(window, cx);
        let CharacterDimensions {
            em_width,
            em_advance,
            line_height,
        } = self.character_dimensions(window, cx);

        let snapshot = self.snapshot(window, cx);
        let scroll_position = snapshot.scroll_position();
        let scroll_left = scroll_position.x * ScrollOffset::from(em_advance);

        let start =
            MultiBufferOffsetUtf16(OffsetUtf16(range_utf16.start)).to_display_point(&snapshot);
        let x = Pixels::from(
            ScrollOffset::from(
                snapshot.x_for_display_point(start, &text_layout_details)
                    + self.gutter_dimensions.full_width(),
            ) - scroll_left,
        );
        let y = line_height * (start.row().as_f64() - scroll_position.y) as f32;

        Some(Bounds {
            origin: element_bounds.origin + point(x, y),
            size: size(em_width, line_height),
        })
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let position_map = self.last_position_map.as_ref()?;
        if !position_map.text_hitbox.contains(&point) {
            return None;
        }
        let display_point = position_map.point_for_position(point).previous_valid;
        let anchor = position_map
            .snapshot
            .display_point_to_anchor(display_point, Bias::Left);
        let utf16_offset = anchor.to_offset_utf16(&position_map.snapshot.buffer_snapshot());
        Some(utf16_offset.0.0)
    }

    fn accepts_text_input(&self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        self.expects_character_input
    }
}

trait SelectionExt {
    fn display_range(&self, map: &DisplaySnapshot) -> Range<DisplayPoint>;
    fn spanned_rows(
        &self,
        include_end_if_at_line_start: bool,
        map: &DisplaySnapshot,
    ) -> Range<MultiBufferRow>;
}

impl<T: ToPoint + ToOffset> SelectionExt for Selection<T> {
    fn display_range(&self, map: &DisplaySnapshot) -> Range<DisplayPoint> {
        let start = self
            .start
            .to_point(map.buffer_snapshot())
            .to_display_point(map);
        let end = self
            .end
            .to_point(map.buffer_snapshot())
            .to_display_point(map);
        if self.reversed {
            end..start
        } else {
            start..end
        }
    }

    fn spanned_rows(
        &self,
        include_end_if_at_line_start: bool,
        map: &DisplaySnapshot,
    ) -> Range<MultiBufferRow> {
        let start = self.start.to_point(map.buffer_snapshot());
        let mut end = self.end.to_point(map.buffer_snapshot());
        if !include_end_if_at_line_start && start.row != end.row && end.column == 0 {
            end.row -= 1;
        }

        let buffer_start = map.prev_line_boundary(start).0;
        let buffer_end = map.next_line_boundary(end).0;
        MultiBufferRow(buffer_start.row)..MultiBufferRow(buffer_end.row + 1)
    }
}

#[derive(Clone)]
struct ErasedEditorImpl(Entity<Editor>);

impl ui_input::ErasedEditor for ErasedEditorImpl {
    fn text(&self, cx: &App) -> String {
        self.0.read(cx).text(cx)
    }

    fn set_text(&self, text: &str, window: &mut Window, cx: &mut App) {
        self.0.update(cx, |this, cx| {
            this.set_text(text, window, cx);
        })
    }

    fn clear(&self, window: &mut Window, cx: &mut App) {
        self.0.update(cx, |this, cx| this.clear(window, cx));
    }

    fn set_placeholder_text(&self, text: &str, window: &mut Window, cx: &mut App) {
        self.0.update(cx, |this, cx| {
            this.set_placeholder_text(text, window, cx);
        });
    }

    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.0.read(cx).focus_handle(cx)
    }

    fn render(&self, _: &mut Window, cx: &App) -> AnyElement {
        let settings = ThemeSettings::get_global(cx);
        let theme_color = cx.theme().colors();

        let text_style = TextStyle {
            font_family: settings.ui_font.family.clone(),
            font_features: settings.ui_font.features.clone(),
            font_size: rems(0.875).into(),
            font_weight: settings.ui_font.weight,
            font_style: FontStyle::Normal,
            line_height: relative(1.2),
            color: theme_color.text,
            ..Default::default()
        };
        let editor_style = EditorStyle {
            background: theme_color.ghost_element_background,
            local_player: cx.theme().players().local(),
            syntax: cx.theme().syntax().clone(),
            text: text_style,
            ..Default::default()
        };
        EditorElement::new(&self.0, editor_style).into_any()
    }

    fn as_any(&self) -> &dyn Any {
        &self.0
    }

    fn move_selection_to_end(&self, window: &mut Window, cx: &mut App) {
        self.0.update(cx, |editor, cx| {
            let editor_offset = editor.buffer().read(cx).len(cx);
            editor.change_selections(
                SelectionEffects::scroll(Autoscroll::Next),
                window,
                cx,
                |s| s.select_ranges(Some(editor_offset..editor_offset)),
            );
        });
    }

    fn subscribe(
        &self,
        mut callback: Box<dyn FnMut(ui_input::ErasedEditorEvent, &mut Window, &mut App) + 'static>,
        window: &mut Window,
        cx: &mut App,
    ) -> Subscription {
        window.subscribe(&self.0, cx, move |_, event: &EditorEvent, window, cx| {
            let event = match event {
                EditorEvent::BufferEdited => ui_input::ErasedEditorEvent::BufferEdited,
                EditorEvent::Blurred => ui_input::ErasedEditorEvent::Blurred,
                _ => return,
            };
            (callback)(event, window, cx);
        })
    }

    fn set_masked(&self, masked: bool, _window: &mut Window, cx: &mut App) {
        self.0.update(cx, |editor, cx| {
            editor.set_masked(masked, cx);
        });
    }
}
pub trait RangeToAnchorExt: Sized {
    fn to_anchors(self, snapshot: &MultiBufferSnapshot) -> Range<Anchor>;

    fn to_display_points(self, snapshot: &EditorSnapshot) -> Range<DisplayPoint> {
        let anchor_range = self.to_anchors(&snapshot.buffer_snapshot());
        anchor_range.start.to_display_point(snapshot)..anchor_range.end.to_display_point(snapshot)
    }
}

impl<T: ToOffset> RangeToAnchorExt for Range<T> {
    fn to_anchors(self, snapshot: &MultiBufferSnapshot) -> Range<Anchor> {
        let start_offset = self.start.to_offset(snapshot);
        let end_offset = self.end.to_offset(snapshot);
        if start_offset == end_offset {
            snapshot.anchor_before(start_offset)..snapshot.anchor_before(end_offset)
        } else {
            snapshot.anchor_after(self.start)..snapshot.anchor_before(self.end)
        }
    }
}

pub trait RowExt {
    fn as_f64(&self) -> f64;

    fn next_row(&self) -> Self;

    fn previous_row(&self) -> Self;

    fn minus(&self, other: Self) -> u32;
}

impl RowExt for DisplayRow {
    fn as_f64(&self) -> f64 {
        self.0 as _
    }

    fn next_row(&self) -> Self {
        Self(self.0 + 1)
    }

    fn previous_row(&self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    fn minus(&self, other: Self) -> u32 {
        self.0 - other.0
    }
}

impl RowExt for MultiBufferRow {
    fn as_f64(&self) -> f64 {
        self.0 as _
    }

    fn next_row(&self) -> Self {
        Self(self.0 + 1)
    }

    fn previous_row(&self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    fn minus(&self, other: Self) -> u32 {
        self.0 - other.0
    }
}

trait RowRangeExt {
    type Row;

    fn len(&self) -> usize;

    fn iter_rows(&self) -> impl DoubleEndedIterator<Item = Self::Row>;
}

impl RowRangeExt for Range<MultiBufferRow> {
    type Row = MultiBufferRow;

    fn len(&self) -> usize {
        (self.end.0 - self.start.0) as usize
    }

    fn iter_rows(&self) -> impl DoubleEndedIterator<Item = MultiBufferRow> {
        (self.start.0..self.end.0).map(MultiBufferRow)
    }
}

impl RowRangeExt for Range<DisplayRow> {
    type Row = DisplayRow;

    fn len(&self) -> usize {
        (self.end.0 - self.start.0) as usize
    }

    fn iter_rows(&self) -> impl DoubleEndedIterator<Item = DisplayRow> {
        (self.start.0..self.end.0).map(DisplayRow)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineHighlight {
    pub background: Background,
    pub border: Option<gpui::Hsla>,
    pub include_gutter: bool,
    pub type_id: Option<TypeId>,
}

pub fn multibuffer_context_lines(cx: &App) -> u32 {
    EditorSettings::try_get(cx)
        .map(|settings| settings.excerpt_context_lines)
        .unwrap_or(2)
        .min(32)
}
