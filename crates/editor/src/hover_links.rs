use crate::{
    Anchor, Editor, EditorSnapshot, HighlightKey, Navigated, PointForPosition, SelectPhase,
};
use gpui::{AsyncWindowContext, Context, Entity, Modifiers, Pixels, Task, Window, px};
use language::{Bias, ToOffset};
use linkify::{LinkFinder, LinkKind};
use project::InlayId;
use std::ops::Range;
use theme::ActiveTheme as _;
use util::{TryFutureExt as _, maybe};

#[derive(Debug)]
pub struct HoveredLinkState {
    pub last_trigger_point: TriggerPoint,
    pub link_range: Option<RangeInEditor>,
    pub links: Vec<HoverLink>,
    pub task: Option<Task<Option<()>>>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum RangeInEditor {
    Text(Range<Anchor>),
}

impl RangeInEditor {
    pub fn point_within_range(
        &self,
        trigger_point: &TriggerPoint,
        snapshot: &EditorSnapshot,
    ) -> bool {
        let Self::Text(range) = self;
        let TriggerPoint::Text(point) = trigger_point;
        let point_after_start = range.start.cmp(point, &snapshot.buffer_snapshot()).is_le();
        point_after_start && range.end.cmp(point, &snapshot.buffer_snapshot()).is_ge()
    }
}

#[derive(Debug, Clone)]
pub enum HoverLink {
    Url(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHighlight {
    pub inlay: InlayId,
    pub inlay_position: Anchor,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TriggerPoint {
    Text(Anchor),
}

impl TriggerPoint {
    fn anchor(&self) -> &Anchor {
        match self {
            TriggerPoint::Text(anchor) => anchor,
        }
    }
}

impl Editor {
    pub(crate) fn update_hovered_link(
        &mut self,
        point_for_position: PointForPosition,
        _mouse_position: Option<gpui::Point<Pixels>>,
        snapshot: &EditorSnapshot,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hovered_link_modifier = Editor::is_cmd_or_ctrl_pressed(&modifiers, cx);
        if !hovered_link_modifier || self.has_pending_selection() || self.mouse_cursor_hidden {
            self.hide_hovered_link(cx);
            return;
        }

        match point_for_position.as_valid() {
            Some(point) => {
                let trigger_point = TriggerPoint::Text(
                    snapshot
                        .buffer_snapshot()
                        .anchor_before(point.to_offset(&snapshot.display_snapshot, Bias::Left)),
                );

                show_link_definition(self, trigger_point, snapshot, window, cx);
            }
            None => self.hide_hovered_link(cx),
        }
    }

    pub(crate) fn hide_hovered_link(&mut self, cx: &mut Context<Self>) {
        self.hovered_link_state.take();
        self.clear_highlights(HighlightKey::HoveredLinkState, cx);
    }

    pub(crate) fn handle_click_hovered_link(
        &mut self,
        point: PointForPosition,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) {
        self.cmd_click_reveal_task(point, modifiers, window, cx)
            .detach_and_log_err(cx);
    }

    fn cmd_click_reveal_task(
        &mut self,
        point: PointForPosition,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Task<anyhow::Result<Navigated>> {
        if let Some(hovered_link_state) = self.hovered_link_state.take() {
            self.hide_hovered_link(cx);
            if !hovered_link_state.links.is_empty() {
                if !self.focus_handle.is_focused(window) {
                    window.focus(&self.focus_handle, cx);
                }

                let split = Self::is_alt_pressed(&modifiers, cx);
                let navigate_task =
                    self.navigate_to_hover_links(hovered_link_state.links, split, window, cx);
                self.select(SelectPhase::End, window, cx);
                return navigate_task;
            }
        }

        // We don't have a link cached, so let the click update the selection.
        self.select(
            SelectPhase::Begin {
                position: point.next_valid,
                add: false,
                click_count: 1,
            },
            window,
            cx,
        );

        self.select(SelectPhase::End, window, cx);
        Task::ready(Ok(Navigated::No))
    }
}

pub fn show_link_definition(
    editor: &mut Editor,
    trigger_point: TriggerPoint,
    snapshot: &EditorSnapshot,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    let (mut hovered_link_state, is_cached) =
        if let Some(existing) = editor.hovered_link_state.take() {
            (existing, true)
        } else {
            (
                HoveredLinkState {
                    last_trigger_point: trigger_point.clone(),
                    link_range: None,
                    links: vec![],
                    task: None,
                },
                false,
            )
        };

    let anchor = trigger_point.anchor().bias_left(snapshot.buffer_snapshot());
    let Some((anchor, _)) = snapshot.buffer_snapshot().anchor_to_buffer_anchor(anchor) else {
        return;
    };
    let Some(buffer) = editor.buffer.read(cx).buffer(anchor.buffer_id) else {
        return;
    };
    if is_cached && (hovered_link_state.last_trigger_point == trigger_point)
        || hovered_link_state
            .link_range
            .as_ref()
            .is_some_and(|link_range| link_range.point_within_range(&trigger_point, snapshot))
    {
        editor.hovered_link_state = Some(hovered_link_state);
        return;
    }
    let snapshot = snapshot.buffer_snapshot().clone();
    hovered_link_state.task = Some(cx.spawn_in(window, async move |this, cx| {
        async move {
            let result = match &trigger_point {
                TriggerPoint::Text(_) => {
                    if let Some((url_range, url)) = find_url(&buffer, anchor, cx.clone()) {
                        this.read_with(cx, |_, _| {
                            let range = maybe!({
                                let range =
                                    snapshot.buffer_anchor_range_to_anchor_range(url_range)?;
                                Some(RangeInEditor::Text(range))
                            });
                            (range, vec![HoverLink::Url(url)])
                        })
                        .ok()
                    } else {
                        None
                    }
                }
            };

            this.update(cx, |editor, cx| {
                // Clear any existing highlights
                editor.clear_highlights(HighlightKey::HoveredLinkState, cx);
                let Some(hovered_link_state) = editor.hovered_link_state.as_mut() else {
                    editor.hide_hovered_link(cx);
                    return;
                };
                hovered_link_state.link_range = result
                    .as_ref()
                    .and_then(|(link_range, _)| link_range.clone());

                if let Some((link_range, links)) = result {
                    hovered_link_state.links = links;

                    if let Some(RangeInEditor::Text(text_range)) = link_range {
                        let style = gpui::HighlightStyle {
                            underline: Some(gpui::UnderlineStyle {
                                thickness: px(1.),
                                ..Default::default()
                            }),
                            color: Some(cx.theme().colors().link_text_hover),
                            ..Default::default()
                        };
                        editor.highlight_text(
                            HighlightKey::HoveredLinkState,
                            vec![text_range],
                            style,
                            cx,
                        )
                    }
                } else {
                    editor.hide_hovered_link(cx);
                }
            })?;

            anyhow::Ok(())
        }
        .log_err()
        .await
    }));

    editor.hovered_link_state = Some(hovered_link_state);
}

pub(crate) fn find_url(
    buffer: &Entity<language::Buffer>,
    position: text::Anchor,
    cx: AsyncWindowContext,
) -> Option<(Range<text::Anchor>, String)> {
    const LIMIT: usize = 2048;

    let snapshot = buffer.read_with(&cx, |buffer, _| buffer.snapshot());

    let offset = position.to_offset(&snapshot);
    let mut token_start = offset;
    let mut token_end = offset;
    let mut found_start = false;
    let mut found_end = false;

    for ch in snapshot.reversed_chars_at(offset).take(LIMIT) {
        if ch.is_whitespace() {
            found_start = true;
            break;
        }
        token_start -= ch.len_utf8();
    }
    // Check if we didn't find the starting whitespace or if we didn't reach the start of the buffer
    if !found_start && token_start != 0 {
        return None;
    }

    for ch in snapshot
        .chars_at(offset)
        .take(LIMIT - (offset - token_start))
    {
        if ch.is_whitespace() {
            found_end = true;
            break;
        }
        token_end += ch.len_utf8();
    }
    // Check if we didn't find the ending whitespace or if we read more or equal than LIMIT
    // which at this point would happen only if we reached the end of buffer
    if !found_end && (token_end - token_start >= LIMIT) {
        return None;
    }

    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]);
    let input = snapshot
        .text_for_range(token_start..token_end)
        .collect::<String>();

    let relative_offset = offset - token_start;
    for link in finder.links(&input) {
        if link.start() <= relative_offset && link.end() >= relative_offset {
            let range = snapshot.anchor_before(token_start + link.start())
                ..snapshot.anchor_after(token_start + link.end());
            return Some((range, link.as_str().to_string()));
        }
    }
    None
}
