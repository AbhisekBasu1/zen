use std::{collections::BTreeMap, ops::Range};

use gpui::{AnyElement, Context, StyledText};
use ui::prelude::*;

use crate::parser::MarkdownEvent;

use super::{Markdown, MarkdownStyle, ParsedMarkdown};

#[derive(Clone, Debug)]
pub(crate) struct ParsedMarkdownMermaidDiagram {
    pub(crate) content_range: Range<usize>,
    pub(crate) contents: ParsedMarkdownMermaidDiagramContents,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ParsedMarkdownMermaidDiagramContents {
    pub(crate) contents: SharedString,
    pub(crate) scale: u32,
}

#[derive(Default, Clone)]
pub(crate) struct MermaidState;

impl MermaidState {
    pub(crate) fn clear(&mut self) {}

    pub(crate) fn update(&mut self, _: &ParsedMarkdown, _: &mut Context<Markdown>) {}
}

pub(crate) fn extract_mermaid_diagrams(
    _: &str,
    _: &[(Range<usize>, MarkdownEvent)],
) -> BTreeMap<usize, ParsedMarkdownMermaidDiagram> {
    BTreeMap::default()
}

pub(crate) fn render_mermaid_diagram(
    parsed: &ParsedMarkdownMermaidDiagram,
    _: &MermaidState,
    style: &MarkdownStyle,
) -> AnyElement {
    let mut container = div().w_full();
    container.style().refine(&style.code_block);
    container
        .child(StyledText::new(parsed.contents.contents.clone()))
        .into_any_element()
}
