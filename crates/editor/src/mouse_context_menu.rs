use crate::{
    Copy, CopyAndTrim, Cut, DisplayPoint, DisplaySnapshot, Editor, Paste, RevealInFileManager,
    SelectMode, SelectionEffects, SelectionExt, ToDisplayPoint,
    selections_collection::SelectionsCollection,
};
use gpui::prelude::FluentBuilder;
use gpui::{Context, DismissEvent, Entity, Focusable as _, Pixels, Point, Subscription, Window};
use std::{ops::Range, path::Path};
use zen_actions::editor::{MarkSelectionAsAgent, MarkSelectionAsHuman, PasteAsAgent};
use zen_actions::preview::markdown::OpenPreview as OpenMarkdownPreview;

#[derive(Debug)]
pub enum MenuPosition {
    /// When the editor is scrolled, the context menu stays on the exact
    /// same position on the screen, never disappearing.
    PinnedToScreen(Point<Pixels>),
    /// When the editor is scrolled, the context menu follows the position it is associated with.
    /// Disappears when the position is no longer visible.
    PinnedToEditor {
        source: multi_buffer::Anchor,
        offset: Point<Pixels>,
    },
}

pub struct MouseContextMenu {
    pub(crate) position: MenuPosition,
    pub(crate) context_menu: Entity<ui::ContextMenu>,
    _dismiss_subscription: Subscription,
    _cursor_move_subscription: Subscription,
}

impl std::fmt::Debug for MouseContextMenu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MouseContextMenu")
            .field("position", &self.position)
            .field("context_menu", &self.context_menu)
            .finish()
    }
}

impl MouseContextMenu {
    pub(crate) fn pinned_to_editor(
        editor: &mut Editor,
        source: multi_buffer::Anchor,
        position: Point<Pixels>,
        context_menu: Entity<ui::ContextMenu>,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Option<Self> {
        let editor_snapshot = editor.snapshot(window, cx);
        let content_origin = editor.last_bounds?.origin
            + Point {
                x: editor.gutter_dimensions.width,
                y: Pixels::ZERO,
            };
        let source_position = editor.to_pixel_point(source, &editor_snapshot, window, cx)?;
        let menu_position = MenuPosition::PinnedToEditor {
            source,
            offset: position - (source_position + content_origin),
        };
        Some(MouseContextMenu::new(
            editor,
            menu_position,
            context_menu,
            window,
            cx,
        ))
    }

    pub(crate) fn new(
        editor: &Editor,
        position: MenuPosition,
        context_menu: Entity<ui::ContextMenu>,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Self {
        let context_menu_focus = context_menu.focus_handle(cx);

        // Since `ContextMenu` is rendered in a deferred fashion its focus
        // handle is not linked to the Editor's until after the deferred draw
        // callback runs.
        // We need to wait for that to happen before focusing it, so that
        // calling `contains_focused` on the editor's focus handle returns
        // `true` when the `ContextMenu` is focused.
        let focus_handle = context_menu_focus.clone();
        cx.on_next_frame(window, move |_, window, cx| {
            cx.on_next_frame(window, move |_, window, cx| {
                window.focus(&focus_handle, cx);
            });
        });

        let _dismiss_subscription = cx.subscribe_in(&context_menu, window, {
            let context_menu_focus = context_menu_focus.clone();
            move |editor, _, _event: &DismissEvent, window, cx| {
                editor.mouse_context_menu.take();
                if context_menu_focus.contains_focused(window, cx) {
                    window.focus(&editor.focus_handle(cx), cx);
                }
            }
        });

        let selection_init = editor.selections.newest_anchor().clone();

        let _cursor_move_subscription = cx.subscribe_in(
            &cx.entity(),
            window,
            move |editor, _, event: &crate::EditorEvent, window, cx| {
                let crate::EditorEvent::SelectionsChanged { local: true } = event else {
                    return;
                };
                let display_snapshot = &editor
                    .display_map
                    .update(cx, |display_map, cx| display_map.snapshot(cx));
                let selection_init_range = selection_init.display_range(display_snapshot);
                let selection_now_range = editor
                    .selections
                    .newest_anchor()
                    .display_range(display_snapshot);
                if selection_now_range == selection_init_range {
                    return;
                }
                editor.mouse_context_menu.take();
                if context_menu_focus.contains_focused(window, cx) {
                    window.focus(&editor.focus_handle(cx), cx);
                }
            },
        );

        Self {
            position,
            context_menu,
            _dismiss_subscription,
            _cursor_move_subscription,
        }
    }
}

fn display_ranges<'a>(
    display_map: &'a DisplaySnapshot,
    selections: &'a SelectionsCollection,
) -> impl Iterator<Item = Range<DisplayPoint>> + 'a {
    let pending = selections.pending_anchor();
    selections
        .disjoint_anchors()
        .iter()
        .chain(pending)
        .map(move |s| s.start.to_display_point(display_map)..s.end.to_display_point(display_map))
}

pub fn deploy_context_menu(
    editor: &mut Editor,
    position: Option<Point<Pixels>>,
    point: DisplayPoint,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    if !editor.is_focused(window) {
        window.focus(&editor.focus_handle(cx), cx);
    }

    let display_map = editor.display_snapshot(cx);
    let source_anchor = display_map.display_point_to_anchor(point, text::Bias::Right);
    let context_menu = if let Some(custom) = editor.custom_context_menu.take() {
        let menu = custom(editor, point, window, cx);
        editor.custom_context_menu = Some(custom);
        let Some(menu) = menu else {
            return;
        };
        menu
    } else {
        // Don't show context menu for inline editors (only applies to default menu)
        if !editor.mode().is_full() {
            return;
        }

        // Don't show the context menu if there isn't a project associated with this editor
        if editor.project.is_none() {
            return;
        }

        let snapshot = editor.snapshot(window, cx);
        let display_map = editor.display_snapshot(cx);
        let buffer = snapshot.buffer_snapshot();
        let anchor = buffer.anchor_before(point.to_point(&display_map));
        if !display_ranges(&display_map, &editor.selections).any(|r| r.contains(&point)) {
            // Move the cursor to the clicked location so that dispatched actions make sense
            editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                s.clear_disjoint();
                s.set_pending_anchor_range(anchor..anchor, SelectMode::Character);
            });
        }

        let focus = window.focused(cx);
        let has_reveal_target = editor.target_file(cx).is_some();
        let has_selection = editor
            .selections
            .disjoint_anchors()
            .iter()
            .any(|selection| selection.start != selection.end);
        let is_markdown = editor
            .buffer()
            .read(cx)
            .as_singleton()
            .is_some_and(|buffer| {
                let buffer = buffer.read(cx);
                buffer
                    .language()
                    .is_some_and(|language| language.name().as_ref() == "Markdown")
                    || buffer
                        .file()
                        .is_some_and(|file| is_markdown_path(&file.full_path(cx)))
            });
        ui::ContextMenu::build(window, cx, |menu, _window, _cx| {
            let builder = menu
                .on_blur_subscription(Subscription::new(|| {}))
                .action("Cut", Box::new(Cut))
                .action("Copy", Box::new(Copy))
                .action("Copy and Trim", Box::new(CopyAndTrim))
                .action("Paste", Box::new(Paste))
                .action("Paste as Agent", Box::new(PasteAsAgent))
                .separator()
                .action_disabled_when(
                    !has_selection,
                    "Mark Selection as Human",
                    Box::new(MarkSelectionAsHuman),
                )
                .action_disabled_when(
                    !has_selection,
                    "Mark Selection as Agent",
                    Box::new(MarkSelectionAsAgent),
                )
                .separator()
                .action_disabled_when(
                    !has_reveal_target,
                    ui::utils::reveal_in_file_manager_label(false),
                    Box::new(RevealInFileManager),
                )
                .when(is_markdown, |builder| {
                    builder.action("Open Markdown Preview", Box::new(OpenMarkdownPreview))
                });
            match focus {
                Some(focus) => builder.context(focus),
                None => builder,
            }
        })
    };

    editor.mouse_context_menu = match position {
        Some(position) => MouseContextMenu::pinned_to_editor(
            editor,
            source_anchor,
            position,
            context_menu,
            window,
            cx,
        ),
        None => {
            let character_size = editor.character_dimensions(window, cx);
            let menu_position = MenuPosition::PinnedToEditor {
                source: source_anchor,
                offset: gpui::point(character_size.em_width, character_size.line_height),
            };
            Some(MouseContextMenu::new(
                editor,
                menu_position,
                context_menu,
                window,
                cx,
            ))
        }
    };
    cx.notify();
}

fn is_markdown_path(path: &Path) -> bool {
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str())
        && matches!(
            extension.to_ascii_lowercase().as_str(),
            "md" | "mdx" | "mdwn" | "mdc" | "markdown"
        )
    {
        return true;
    }

    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file_name| {
            matches!(
                file_name,
                "README" | "CHANGELOG" | "CONTRIBUTING" | "LICENSE" | "NEWS" | "NOTICE"
            )
        })
}
