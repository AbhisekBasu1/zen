use crate::{Editor, HighlightKey, RangeToAnchorExt, display_map::DisplaySnapshot};
use gpui::{AppContext, Context, HighlightStyle};
use language::CursorShape;
use multi_buffer::MultiBufferOffset;
use theme::ActiveTheme;

impl Editor {
    #[ztracing::instrument(skip_all)]
    pub fn refresh_matching_bracket_highlights(
        &mut self,
        snapshot: &DisplaySnapshot,
        cx: &mut Context<Editor>,
    ) {
        let newest_selection = self.selections.newest::<MultiBufferOffset>(&snapshot);
        // Don't highlight brackets if the selection isn't empty
        if !newest_selection.is_empty() {
            self.clear_highlights(HighlightKey::MatchingBracket, cx);
            return;
        }

        let buffer_snapshot = snapshot.buffer_snapshot();
        let head = newest_selection.head();
        if head > buffer_snapshot.len() {
            log::error!("bug: cursor offset is out of range while refreshing bracket highlights");
            return;
        }

        let mut tail = head;
        if (self.cursor_shape == CursorShape::Block || self.cursor_shape == CursorShape::Hollow)
            && head < buffer_snapshot.len()
        {
            if let Some(tail_ch) = buffer_snapshot.chars_at(tail).next() {
                tail += tail_ch.len_utf8();
            }
        }
        let task = cx.background_spawn({
            let buffer_snapshot = buffer_snapshot.clone();
            async move { buffer_snapshot.innermost_enclosing_bracket_ranges(head..tail, None) }
        });
        self.refresh_matching_bracket_highlights_task = cx.spawn({
            let buffer_snapshot = buffer_snapshot.clone();
            async move |this, cx| {
                let bracket_ranges = task.await;
                let current_ranges = this
                    .read_with(cx, |editor, cx| {
                        editor
                            .display_map
                            .read(cx)
                            .text_highlights(HighlightKey::MatchingBracket)
                            .map(|(_, ranges)| ranges.to_vec())
                    })
                    .ok()
                    .flatten();
                let new_ranges = bracket_ranges.map(|(opening_range, closing_range)| {
                    vec![
                        opening_range.to_anchors(&buffer_snapshot),
                        closing_range.to_anchors(&buffer_snapshot),
                    ]
                });

                if current_ranges != new_ranges {
                    this.update(cx, |editor, cx| {
                        editor.clear_highlights(HighlightKey::MatchingBracket, cx);
                        if let Some(new_ranges) = new_ranges {
                            editor.highlight_text(
                                HighlightKey::MatchingBracket,
                                new_ranges,
                                HighlightStyle {
                                    background_color: Some(
                                        cx.theme()
                                            .colors()
                                            .editor_document_highlight_bracket_background,
                                    ),
                                    ..Default::default()
                                },
                                cx,
                            )
                        }
                    })
                    .ok();
                }
            }
        });
    }
}
