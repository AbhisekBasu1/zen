use crate::Tooltip;
use crate::prelude::*;

#[derive(IntoElement)]
pub struct DiffStat {
    id: ElementId,
    added: usize,
    removed: usize,
    label_size: LabelSize,
    tooltip: Option<SharedString>,
}

impl DiffStat {
    pub fn new(id: impl Into<ElementId>, added: usize, removed: usize) -> Self {
        Self {
            id: id.into(),
            added,
            removed,
            label_size: LabelSize::Small,
            tooltip: None,
        }
    }

    pub fn label_size(mut self, label_size: LabelSize) -> Self {
        self.label_size = label_size;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }
}

impl RenderOnce for DiffStat {
    fn render(self, _: &mut Window, _cx: &mut App) -> impl IntoElement {
        let tooltip = self.tooltip;
        h_flex()
            .id(self.id)
            .gap_1()
            .child(
                Label::new(format!("+\u{2009}{}", self.added))
                    .color(Color::Success)
                    .size(self.label_size),
            )
            .child(
                Label::new(format!("\u{2012}\u{2009}{}", self.removed))
                    .color(Color::Error)
                    .size(self.label_size),
            )
            .when_some(tooltip, |this, tooltip| {
                this.tooltip(Tooltip::text(tooltip))
            })
    }
}
