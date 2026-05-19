use gpui::{Hsla, IntoElement, PathBuilder, canvas, point};

use crate::prelude::*;

pub fn divider() -> Divider {
    Divider {
        style: DividerStyle::Solid,
        direction: DividerDirection::Horizontal,
        color: DividerColor::default(),
        inset: false,
    }
}

pub fn vertical_divider() -> Divider {
    Divider {
        style: DividerStyle::Solid,
        direction: DividerDirection::Vertical,
        color: DividerColor::default(),
        inset: false,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum DividerStyle {
    Solid,
    Dashed,
}

#[derive(Clone, Copy, PartialEq)]
enum DividerDirection {
    Horizontal,
    Vertical,
}

/// The color of a [`Divider`].
#[derive(Default)]
pub enum DividerColor {
    Border,
    BorderFaded,
    #[default]
    BorderVariant,
}

impl DividerColor {
    pub fn hsla(self, cx: &mut App) -> Hsla {
        match self {
            DividerColor::Border => cx.theme().colors().border,
            DividerColor::BorderFaded => cx.theme().colors().border.opacity(0.6),
            DividerColor::BorderVariant => cx.theme().colors().border_variant,
        }
    }
}

#[derive(IntoElement)]
pub struct Divider {
    style: DividerStyle,
    direction: DividerDirection,
    color: DividerColor,
    inset: bool,
}

impl Divider {
    pub fn horizontal() -> Self {
        Self {
            style: DividerStyle::Solid,
            direction: DividerDirection::Horizontal,
            color: DividerColor::default(),
            inset: false,
        }
    }

    pub fn vertical() -> Self {
        Self {
            style: DividerStyle::Solid,
            direction: DividerDirection::Vertical,
            color: DividerColor::default(),
            inset: false,
        }
    }

    pub fn horizontal_dashed() -> Self {
        Self {
            style: DividerStyle::Dashed,
            direction: DividerDirection::Horizontal,
            color: DividerColor::default(),
            inset: false,
        }
    }

    pub fn vertical_dashed() -> Self {
        Self {
            style: DividerStyle::Dashed,
            direction: DividerDirection::Vertical,
            color: DividerColor::default(),
            inset: false,
        }
    }

    pub fn inset(mut self) -> Self {
        self.inset = true;
        self
    }

    pub fn color(mut self, color: DividerColor) -> Self {
        self.color = color;
        self
    }

    pub fn render_solid(self, base: Div, cx: &mut App) -> impl IntoElement {
        base.bg(self.color.hsla(cx))
    }

    pub fn render_dashed(self, base: Div) -> impl IntoElement {
        base.relative().child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, cx| {
                    let mut builder = PathBuilder::stroke(px(1.)).dash_array(&[px(4.), px(2.)]);
                    let (start, end) = match self.direction {
                        DividerDirection::Horizontal => {
                            let x = bounds.origin.x;
                            let y = bounds.origin.y + px(0.5);
                            (point(x, y), point(x + bounds.size.width, y))
                        }
                        DividerDirection::Vertical => {
                            let x = bounds.origin.x + px(0.5);
                            let y = bounds.origin.y;
                            (point(x, y), point(x, y + bounds.size.height))
                        }
                    };
                    builder.move_to(start);
                    builder.line_to(end);
                    if let Ok(line) = builder.build() {
                        window.paint_path(line, self.color.hsla(cx));
                    }
                },
            )
            .absolute()
            .size_full(),
        )
    }
}

impl RenderOnce for Divider {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let base = match self.direction {
            DividerDirection::Horizontal => div()
                .min_w_0()
                .h_px()
                .w_full()
                .when(self.inset, |this| this.mx_1p5()),
            DividerDirection::Vertical => div()
                .min_w_0()
                .w_px()
                .h_full()
                .when(self.inset, |this| this.my_1p5()),
        };

        match self.style {
            DividerStyle::Solid => self.render_solid(base, cx).into_any_element(),
            DividerStyle::Dashed => self.render_dashed(base).into_any_element(),
        }
    }
}
