use crate::Editor;
use gpui::{Context, Pixels, Window, px};

pub const MIN_POPOVER_CHARACTER_WIDTH: f32 = 20.;
pub const MIN_POPOVER_LINE_HEIGHT: f32 = 4.;
pub const POPOVER_RIGHT_OFFSET: Pixels = px(8.0);
pub const HOVER_POPOVER_GAP: Pixels = px(10.);

#[derive(Default)]
pub struct HoverState;

impl HoverState {
    pub fn focused(&self, _: &mut Window, _: &mut Context<Editor>) -> bool {
        false
    }
}

pub fn hide_hover(_: &mut Editor, _: &mut Context<Editor>) -> bool {
    false
}
