use text::BufferId;
use ui::{Context, Window};

use crate::Editor;

impl Editor {
    pub(super) fn refresh_folding_ranges(
        &mut self,
        _for_buffer: Option<BufferId>,
        _window: &Window,
        _cx: &mut Context<Self>,
    ) {
    }
}
