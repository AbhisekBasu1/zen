use gpui::IntoElement;

use crate::{IconDecoration, prelude::*};

#[derive(IntoElement)]
pub struct DecoratedIcon {
    icon: Icon,
    decoration: Option<IconDecoration>,
}

impl DecoratedIcon {
    pub fn new(icon: Icon, decoration: Option<IconDecoration>) -> Self {
        Self { icon, decoration }
    }
}

impl RenderOnce for DecoratedIcon {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .relative()
            .size(self.icon.size)
            .child(self.icon)
            .children(self.decoration)
    }
}
