use crate::Editor;
use gpui::{
    App, Context, FontWeight, Pixels, SharedString, StyleRefinement, TextStyleRefinement, Window,
    px,
};
use markdown::MarkdownStyle;
use settings::Settings;
use std::path::PathBuf;
use theme_settings::ThemeSettings;
use ui::prelude::*;
use url::Url;
use workspace::{OpenOptions, OpenVisible, Workspace};

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

pub fn hover_markdown_style(window: &Window, cx: &App) -> MarkdownStyle {
    let settings = ThemeSettings::get_global(cx);
    let ui_font_family = settings.ui_font.family.clone();
    let ui_font_features = settings.ui_font.features.clone();
    let ui_font_fallbacks = settings.ui_font.fallbacks.clone();
    let buffer_font_family = settings.buffer_font.family.clone();
    let buffer_font_features = settings.buffer_font.features.clone();
    let buffer_font_fallbacks = settings.buffer_font.fallbacks.clone();
    let buffer_font_weight = settings.buffer_font.weight;

    let mut base_text_style = window.text_style();
    base_text_style.refine(&TextStyleRefinement {
        font_family: Some(ui_font_family),
        font_features: Some(ui_font_features),
        font_fallbacks: ui_font_fallbacks,
        color: Some(cx.theme().colors().editor_foreground),
        ..Default::default()
    });
    MarkdownStyle {
        base_text_style,
        code_block: StyleRefinement::default()
            .my(rems(1.))
            .font_buffer(cx)
            .font_features(buffer_font_features.clone())
            .font_weight(buffer_font_weight),
        inline_code: TextStyleRefinement {
            background_color: Some(cx.theme().colors().background),
            font_family: Some(buffer_font_family),
            font_features: Some(buffer_font_features),
            font_fallbacks: buffer_font_fallbacks,
            font_weight: Some(buffer_font_weight),
            ..Default::default()
        },
        rule_color: cx.theme().colors().border,
        block_quote_border_color: Color::Muted.color(cx),
        block_quote: TextStyleRefinement {
            color: Some(Color::Muted.color(cx)),
            ..Default::default()
        },
        link: TextStyleRefinement {
            color: Some(cx.theme().colors().editor_foreground),
            underline: Some(gpui::UnderlineStyle {
                thickness: px(1.),
                color: Some(cx.theme().colors().editor_foreground),
                wavy: false,
            }),
            ..Default::default()
        },
        syntax: cx.theme().syntax().clone(),
        selection_background_color: cx.theme().colors().element_selection_background,
        heading: StyleRefinement::default()
            .font_weight(FontWeight::BOLD)
            .text_base()
            .mt(rems(1.))
            .mb_0(),
        table_columns_min_size: true,
        ..Default::default()
    }
}

pub fn open_markdown_url(link: SharedString, window: &mut Window, cx: &mut App) {
    if let Ok(uri) = Url::parse(&link)
        && uri.scheme() == "file"
        && let Some(workspace) = Workspace::for_window(window, cx)
    {
        workspace.update(cx, |workspace, cx| {
            let task = workspace.open_abs_path(
                PathBuf::from(uri.path()),
                OpenOptions {
                    visible: Some(OpenVisible::None),
                    ..Default::default()
                },
                window,
                cx,
            );

            cx.spawn_in(window, async move |_, cx| {
                let item = task.await?;
                let Some(fragment) = uri.fragment() else {
                    return anyhow::Ok(());
                };
                let mut accum = 0u32;
                for char in fragment.chars() {
                    if char.is_ascii_digit() && accum < u32::MAX / 2 {
                        accum *= 10;
                        accum += char as u32 - '0' as u32;
                    } else if accum > 0 {
                        break;
                    }
                }
                if accum == 0 {
                    return Ok(());
                }
                let Some(editor) = cx.update(|_, cx| item.act_as::<Editor>(cx))? else {
                    return Ok(());
                };
                editor.update_in(cx, |editor, window, cx| {
                    editor.change_selections(Default::default(), window, cx, |selections| {
                        selections.select_ranges([
                            text::Point::new(accum - 1, 0)..text::Point::new(accum - 1, 0)
                        ]);
                    });
                })
            })
            .detach_and_log_err(cx);
        });
        return;
    }
    cx.open_url(&link);
}
