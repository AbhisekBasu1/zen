//! The logic, responsible for managing [`Inlay`]s in the editor.
//!
//! Inlays are "not real" text that gets mixed into the "real" buffer's text.
//! They are attached to a certain [`Anchor`], and display certain contents (usually, strings)
//! between real text around that anchor.
//!
//! Inlays are used for virtual text such as inline predictions.
//!
//! Editor uses [`crate::DisplayMap`] and [`crate::display_map::InlayMap`] to manage what's rendered inside the editor.

use gpui::Context;
use multi_buffer::Anchor;
use project::InlayId;
use text::Rope;

use crate::Editor;

#[derive(Debug, Clone)]
pub struct Inlay {
    pub id: InlayId,
    // TODO this could be an ExcerptAnchor
    pub position: Anchor,
    pub content: InlayContent,
}

#[derive(Debug, Clone)]
pub enum InlayContent {
    Text(text::Rope),
}

impl Inlay {
    #[cfg(any(test, feature = "test-support"))]
    pub fn mock_hint(id: usize, position: Anchor, text: impl Into<Rope>) -> Self {
        Self {
            id: InlayId::Hint(id),
            position,
            content: InlayContent::Text(text.into()),
        }
    }

    pub fn edit_prediction<T: Into<Rope>>(id: usize, position: Anchor, text: T) -> Self {
        Self {
            id: InlayId::EditPrediction(id),
            position,
            content: InlayContent::Text(text.into()),
        }
    }

    pub fn text(&self) -> &Rope {
        match &self.content {
            InlayContent::Text(text) => text,
        }
    }
}

impl Editor {
    /// Modify which hints are displayed in the editor.
    pub fn splice_inlays(
        &mut self,
        to_remove: &[InlayId],
        to_insert: Vec<Inlay>,
        cx: &mut Context<Self>,
    ) {
        self.display_map.update(cx, |display_map, cx| {
            display_map.splice_inlays(to_remove, to_insert, cx)
        });
        cx.notify();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn all_inlays(&self, cx: &gpui::App) -> Vec<Inlay> {
        self.display_map
            .read(cx)
            .current_inlays()
            .cloned()
            .collect()
    }
}
