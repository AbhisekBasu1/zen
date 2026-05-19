use gpui::FontWeight;

use crate::prelude::*;

/// A small, pill-shaped badge that displays a numeric count.
///
/// The count is capped at 99 and displayed as "99+" beyond that.
#[derive(IntoElement)]
pub struct CountBadge {
    count: usize,
}

impl CountBadge {
    pub fn new(count: usize) -> Self {
        Self { count }
    }
}

impl RenderOnce for CountBadge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let label = if self.count > 99 {
            "99+".to_string()
        } else {
            self.count.to_string()
        };

        let bg = cx
            .theme()
            .colors()
            .editor_background
            .blend(cx.theme().status().error.opacity(0.4));

        h_flex()
            .absolute()
            .top_0()
            .right_0()
            .p_px()
            .h_3p5()
            .min_w_3p5()
            .rounded_full()
            .justify_center()
            .text_center()
            .border_1()
            .border_color(cx.theme().colors().border)
            .bg(bg)
            .shadow_sm()
            .child(
                Label::new(label)
                    .size(LabelSize::Custom(rems_from_px(9.)))
                    .weight(FontWeight::MEDIUM),
            )
    }
}
