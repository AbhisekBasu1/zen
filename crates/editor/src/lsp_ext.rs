use std::sync::Arc;

use crate::Editor;
use collections::HashSet;
use gpui::{App, Entity};
use language::{Buffer, Language};
use lsp::{LanguageServerId, LanguageServerName};

pub(crate) fn find_specific_language_server_in_selection<F>(
    editor: &Editor,
    cx: &mut App,
    filter_language: F,
    language_server_name: LanguageServerName,
) -> Option<(
    text::Anchor,
    Arc<Language>,
    LanguageServerId,
    Entity<Buffer>,
)>
where
    F: Fn(&Language) -> bool,
{
    let project = editor.project.clone()?;
    let multi_buffer = editor.buffer();
    let mut seen_buffer_ids = HashSet::default();
    editor
        .selections
        .disjoint_anchors_arc()
        .iter()
        .find_map(|selection| {
            let multi_buffer = multi_buffer.read(cx);
            let multi_buffer_snapshot = multi_buffer.snapshot(cx);
            let (position, buffer) = multi_buffer_snapshot
                .anchor_to_buffer_anchor(selection.head())
                .and_then(|(anchor, _)| Some((anchor, multi_buffer.buffer(anchor.buffer_id)?)))?;
            if !seen_buffer_ids.insert(buffer.read(cx).remote_id()) {
                return None;
            }

            let language = buffer.read(cx).language_at(position)?;
            if filter_language(&language) {
                let server_id = buffer.update(cx, |buffer, cx| {
                    project
                        .read(cx)
                        .language_server_id_for_name(buffer, &language_server_name, cx)
                })?;
                Some((position, language, server_id, buffer))
            } else {
                None
            }
        })
}
