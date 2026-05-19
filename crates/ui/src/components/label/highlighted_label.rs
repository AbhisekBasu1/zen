use std::ops::Range;

use gpui::{FontWeight, HighlightStyle, StyleRefinement, StyledText};

use crate::{LabelCommon, LabelLike, LabelSize, LineHeightStyle, prelude::*};

#[derive(IntoElement)]
pub struct HighlightedLabel {
    base: LabelLike,
    label: SharedString,
    highlight_indices: Vec<usize>,
}

impl HighlightedLabel {
    /// Constructs a label with the given characters highlighted.
    /// Characters are identified by UTF-8 byte position.
    pub fn new(label: impl Into<SharedString>, highlight_indices: Vec<usize>) -> Self {
        let label = label.into();
        for &run in &highlight_indices {
            assert!(
                label.is_char_boundary(run),
                "highlight index {run} is not a valid UTF-8 boundary"
            );
        }
        Self {
            base: LabelLike::new(),
            label,
            highlight_indices,
        }
    }

    /// Constructs a label with the given byte ranges highlighted.
    /// Assumes that the highlight ranges are valid UTF-8 byte positions.
    pub fn from_ranges(
        label: impl Into<SharedString>,
        highlight_ranges: Vec<Range<usize>>,
    ) -> Self {
        let label = label.into();
        let highlight_indices = highlight_ranges
            .iter()
            .flat_map(|range| {
                let mut indices = Vec::new();
                let mut index = range.start;
                while index < range.end {
                    indices.push(index);
                    index += label[index..].chars().next().map_or(0, |c| c.len_utf8());
                }
                indices
            })
            .collect();

        Self {
            base: LabelLike::new(),
            label,
            highlight_indices,
        }
    }

    pub fn text(&self) -> &str {
        self.label.as_str()
    }

    pub fn highlight_indices(&self) -> &[usize] {
        &self.highlight_indices
    }
}

impl HighlightedLabel {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.base.style()
    }

    pub fn flex_1(mut self) -> Self {
        self.style().flex_grow = Some(1.);
        self.style().flex_shrink = Some(1.);
        self.style().flex_basis = Some(gpui::relative(0.).into());
        self
    }

    pub fn flex_none(mut self) -> Self {
        self.style().flex_grow = Some(0.);
        self.style().flex_shrink = Some(0.);
        self
    }

    pub fn flex_grow(mut self) -> Self {
        self.style().flex_grow = Some(1.);
        self
    }

    pub fn flex_shrink(mut self) -> Self {
        self.style().flex_shrink = Some(1.);
        self
    }

    pub fn flex_shrink_0(mut self) -> Self {
        self.style().flex_shrink = Some(0.);
        self
    }
}

impl LabelCommon for HighlightedLabel {
    fn size(mut self, size: LabelSize) -> Self {
        self.base = self.base.size(size);
        self
    }

    fn weight(mut self, weight: FontWeight) -> Self {
        self.base = self.base.weight(weight);
        self
    }

    fn line_height_style(mut self, line_height_style: LineHeightStyle) -> Self {
        self.base = self.base.line_height_style(line_height_style);
        self
    }

    fn color(mut self, color: Color) -> Self {
        self.base = self.base.color(color);
        self
    }

    fn strikethrough(mut self) -> Self {
        self.base = self.base.strikethrough();
        self
    }

    fn italic(mut self) -> Self {
        self.base = self.base.italic();
        self
    }

    fn alpha(mut self, alpha: f32) -> Self {
        self.base = self.base.alpha(alpha);
        self
    }

    fn underline(mut self) -> Self {
        self.base = self.base.underline();
        self
    }

    fn truncate(mut self) -> Self {
        self.base = self.base.truncate();
        self
    }

    fn single_line(mut self) -> Self {
        self.base = self.base.single_line();
        self
    }

    fn buffer_font(mut self, cx: &App) -> Self {
        self.base = self.base.buffer_font(cx);
        self
    }

    fn inline_code(mut self, cx: &App) -> Self {
        self.base = self.base.inline_code(cx);
        self
    }
}

pub fn highlight_ranges(
    text: &str,
    indices: &[usize],
    style: HighlightStyle,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut highlight_indices = indices.iter().copied().peekable();
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();

    while let Some(start_ix) = highlight_indices.next() {
        let mut end_ix = start_ix;

        loop {
            end_ix += text[end_ix..].chars().next().map_or(0, |c| c.len_utf8());
            if highlight_indices.next_if(|&ix| ix == end_ix).is_none() {
                break;
            }
        }

        highlights.push((start_ix..end_ix, style));
    }

    highlights
}

impl RenderOnce for HighlightedLabel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let highlight_color = cx.theme().colors().text_accent;

        let highlights = highlight_ranges(
            &self.label,
            &self.highlight_indices,
            HighlightStyle {
                color: Some(highlight_color),
                ..Default::default()
            },
        );

        let mut text_style = window.text_style();
        text_style.color = self.base.color.color(cx);

        self.base
            .child(StyledText::new(self.label).with_default_highlights(&text_style, highlights))
    }
}
