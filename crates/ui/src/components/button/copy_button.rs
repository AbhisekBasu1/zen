use std::time::{Duration, Instant};

use gpui::{App, ClipboardItem, Context, ElementId, Entity, IntoElement, RenderOnce, Window};

use crate::{Tooltip, prelude::*};

const COPIED_STATE_DURATION: Duration = Duration::from_secs(2);

struct CopyButtonState {
    copied_at: Option<Instant>,
}

impl CopyButtonState {
    fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { copied_at: None }
    }

    fn is_copied(&self) -> bool {
        self.copied_at
            .map(|t| t.elapsed() < COPIED_STATE_DURATION)
            .unwrap_or(false)
    }

    fn mark_copied(&mut self) {
        self.copied_at = Some(Instant::now());
    }
}

#[derive(IntoElement)]
pub struct CopyButton {
    id: ElementId,
    message: SharedString,
    icon_size: IconSize,
    disabled: bool,
    tooltip_label: SharedString,
    visible_on_hover: Option<SharedString>,
    custom_on_click: Option<Box<dyn Fn(&mut Window, &mut App) + 'static>>,
}

impl CopyButton {
    pub fn new(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            message: message.into(),
            icon_size: IconSize::Small,
            disabled: false,
            tooltip_label: "Copy".into(),
            visible_on_hover: None,
            custom_on_click: None,
        }
    }

    pub fn icon_size(mut self, icon_size: IconSize) -> Self {
        self.icon_size = icon_size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn tooltip_label(mut self, tooltip_label: impl Into<SharedString>) -> Self {
        self.tooltip_label = tooltip_label.into();
        self
    }

    pub fn visible_on_hover(mut self, visible_on_hover: impl Into<SharedString>) -> Self {
        self.visible_on_hover = Some(visible_on_hover.into());
        self
    }

    pub fn custom_on_click(
        mut self,
        custom_on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.custom_on_click = Some(Box::new(custom_on_click));
        self
    }
}

impl RenderOnce for CopyButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.id.clone();
        let message = self.message;
        let custom_on_click = self.custom_on_click;
        let visible_on_hover = self.visible_on_hover;

        let state: Entity<CopyButtonState> =
            window.use_keyed_state(id.clone(), cx, CopyButtonState::new);
        let is_copied = state.read(cx).is_copied();

        let (icon, color, tooltip) = if is_copied {
            (IconName::Check, Color::Success, "Copied!".into())
        } else {
            (IconName::Copy, Color::Muted, self.tooltip_label)
        };

        let button = IconButton::new(id, icon)
            .icon_color(color)
            .icon_size(self.icon_size)
            .disabled(self.disabled)
            .tooltip(Tooltip::text(tooltip))
            .on_click(move |_, window, cx| {
                state.update(cx, |state, _cx| {
                    state.mark_copied();
                });

                if let Some(custom_on_click) = custom_on_click.as_ref() {
                    (custom_on_click)(window, cx);
                } else {
                    cx.stop_propagation();
                    cx.write_to_clipboard(ClipboardItem::new_string(message.to_string()));
                }

                let state_id = state.entity_id();
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(COPIED_STATE_DURATION).await;
                    cx.update(|cx| {
                        cx.notify(state_id);
                    })
                })
                .detach();
            });

        if let Some(visible_on_hover) = visible_on_hover {
            button.visible_on_hover(visible_on_hover)
        } else {
            button
        }
    }
}
