//! Bracket highlights, also known as "rainbow brackets".
//! Uses tree-sitter queries from brackets.scm to capture bracket pairs,
//! and theme accents to colorize those.

use std::ops::Range;

use crate::{Editor, HighlightKey};
use collections::{HashMap, HashSet};
use gpui::{AppContext as _, Context, HighlightStyle};
use language::{BufferRow, BufferSnapshot, language_settings::LanguageSettings};
use multi_buffer::{Anchor, BufferOffset, ExcerptRange, MultiBufferSnapshot};
use text::OffsetRangeExt as _;
use ui::{ActiveTheme, utils::ensure_minimum_contrast};

impl Editor {
    pub(crate) fn colorize_brackets(&mut self, invalidate: bool, cx: &mut Context<Editor>) {
        if !self.mode.is_full() {
            return;
        }

        if invalidate {
            self.bracket_fetched_tree_sitter_chunks.clear();
        }

        let accents_count = cx.theme().accents().0.len();
        let multi_buffer_snapshot = self.buffer().read(cx).snapshot(cx);

        let visible_excerpts = self.visible_buffer_ranges(cx);
        let excerpt_data: Vec<(
            BufferSnapshot,
            Range<BufferOffset>,
            ExcerptRange<text::Anchor>,
        )> = visible_excerpts
            .into_iter()
            .filter(|(buffer_snapshot, _, _)| {
                let Some(buffer) = self.buffer().read(cx).buffer(buffer_snapshot.remote_id())
                else {
                    return false;
                };
                LanguageSettings::for_buffer(buffer.read(cx), cx).colorize_brackets
            })
            .collect();

        let mut fetched_tree_sitter_chunks = excerpt_data
            .iter()
            .filter_map(|(_, _, excerpt_range)| {
                let key = excerpt_range.context.clone();
                Some((
                    key.clone(),
                    self.bracket_fetched_tree_sitter_chunks.get(&key).cloned()?,
                ))
            })
            .collect::<HashMap<Range<text::Anchor>, HashSet<Range<BufferRow>>>>();

        let bracket_matches_by_accent = cx.background_spawn(async move {
            let bracket_matches_by_accent: HashMap<usize, Vec<Range<Anchor>>> =
                excerpt_data.into_iter().fold(
                    HashMap::default(),
                    |mut acc, (buffer_snapshot, buffer_range, excerpt_range)| {
                        let fetched_chunks = fetched_tree_sitter_chunks
                            .entry(excerpt_range.context.clone())
                            .or_default();

                        let brackets_by_accent = compute_bracket_ranges(
                            &multi_buffer_snapshot,
                            &buffer_snapshot,
                            buffer_range,
                            excerpt_range,
                            fetched_chunks,
                            accents_count,
                        );

                        for (accent_number, new_ranges) in brackets_by_accent {
                            let ranges = acc
                                .entry(accent_number)
                                .or_insert_with(Vec::<Range<Anchor>>::new);

                            for new_range in new_ranges {
                                let i = ranges
                                    .binary_search_by(|probe| {
                                        probe.start.cmp(&new_range.start, &multi_buffer_snapshot)
                                    })
                                    .unwrap_or_else(|i| i);
                                ranges.insert(i, new_range);
                            }
                        }

                        acc
                    },
                );

            (bracket_matches_by_accent, fetched_tree_sitter_chunks)
        });

        let editor_background = cx.theme().colors().editor_background;
        let accents = cx.theme().accents().clone();

        self.colorize_brackets_task = cx.spawn(async move |editor, cx| {
            if invalidate {
                editor
                    .update(cx, |editor, cx| {
                        editor.clear_highlights_with(
                            &mut |key| matches!(key, HighlightKey::ColorizeBracket(_)),
                            cx,
                        );
                    })
                    .ok();
            }

            let (bracket_matches_by_accent, updated_chunks) = bracket_matches_by_accent.await;

            editor
                .update(cx, |editor, cx| {
                    editor
                        .bracket_fetched_tree_sitter_chunks
                        .extend(updated_chunks);
                    for (accent_number, bracket_highlights) in bracket_matches_by_accent {
                        let bracket_color = accents.color_for_index(accent_number as u32);
                        let adjusted_color =
                            ensure_minimum_contrast(bracket_color, editor_background, 55.0);
                        let style = HighlightStyle {
                            color: Some(adjusted_color),
                            ..HighlightStyle::default()
                        };

                        editor.highlight_text_key(
                            HighlightKey::ColorizeBracket(accent_number),
                            bracket_highlights,
                            style,
                            true,
                            cx,
                        );
                    }
                })
                .ok();
        });
    }
}

fn compute_bracket_ranges(
    multi_buffer_snapshot: &MultiBufferSnapshot,
    buffer_snapshot: &BufferSnapshot,
    buffer_range: Range<BufferOffset>,
    excerpt_range: ExcerptRange<text::Anchor>,
    fetched_chunks: &mut HashSet<Range<BufferRow>>,
    accents_count: usize,
) -> Vec<(usize, Vec<Range<Anchor>>)> {
    let context = excerpt_range.context.to_offset(buffer_snapshot);

    buffer_snapshot
        .fetch_bracket_ranges(
            buffer_range.start.0..buffer_range.end.0,
            Some(fetched_chunks),
        )
        .into_iter()
        .flat_map(|(chunk_range, pairs)| {
            if fetched_chunks.insert(chunk_range) {
                pairs
            } else {
                Vec::new()
            }
        })
        .filter_map(|pair| {
            let color_index = pair.color_index?;

            let mut ranges = Vec::new();

            if context.start <= pair.open_range.start && pair.open_range.end <= context.end {
                let anchors = buffer_snapshot.anchor_range_inside(pair.open_range);
                ranges.push(
                    multi_buffer_snapshot.anchor_in_buffer(anchors.start)?
                        ..multi_buffer_snapshot.anchor_in_buffer(anchors.end)?,
                );
            };

            if context.start <= pair.close_range.start && pair.close_range.end <= context.end {
                let anchors = buffer_snapshot.anchor_range_inside(pair.close_range);
                ranges.push(
                    multi_buffer_snapshot.anchor_in_buffer(anchors.start)?
                        ..multi_buffer_snapshot.anchor_in_buffer(anchors.end)?,
                );
            };

            Some((color_index % accents_count, ranges))
        })
        .collect()
}
