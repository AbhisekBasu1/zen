use gpui::{App, Context};
use project::lsp_store::RefreshForServer;
use settings::SemanticTokenRules;
use text::BufferId;

use crate::{Editor, actions::ToggleSemanticHighlights};

pub(super) struct SemanticTokenState;

impl SemanticTokenState {
    pub(super) fn new(_cx: &App, _enabled: bool) -> Self {
        Self
    }

    pub(super) fn enabled(&self) -> bool {
        false
    }

    pub(super) fn invalidate_buffer(&mut self, _buffer_id: &BufferId) {}

    pub(super) fn update_rules(&mut self, _new_rules: SemanticTokenRules) -> bool {
        false
    }
}

impl Editor {
    pub fn supports_semantic_tokens(&self, _cx: &mut App) -> bool {
        false
    }

    pub fn semantic_highlights_enabled(&self) -> bool {
        false
    }

    pub fn toggle_semantic_highlights(
        &mut self,
        _: &ToggleSemanticHighlights,
        _window: &mut gpui::Window,
        _cx: &mut Context<Self>,
    ) {
    }

    pub(super) fn invalidate_semantic_tokens(&mut self, _for_buffer: Option<BufferId>) {}

    pub(super) fn refresh_semantic_tokens(
        &mut self,
        _buffer_id: Option<BufferId>,
        _for_server: Option<RefreshForServer>,
        _cx: &mut Context<Self>,
    ) {
    }
}
