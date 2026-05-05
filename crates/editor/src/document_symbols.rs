use gpui::{Context, Task};
use language::OutlineItem;
use text::BufferId;
use theme::ActiveTheme;

use crate::Editor;

impl Editor {
    pub fn buffer_outline_items(
        &self,
        buffer_id: BufferId,
        cx: &mut Context<Self>,
    ) -> Task<Vec<OutlineItem<text::Anchor>>> {
        let Some(buffer) = self.buffer.read(cx).buffer(buffer_id) else {
            return Task::ready(Vec::new());
        };

        let buffer_snapshot = buffer.read(cx).snapshot();
        let syntax = cx.theme().syntax().clone();
        cx.background_executor()
            .spawn(async move { buffer_snapshot.outline(Some(&syntax)).items })
    }
}
