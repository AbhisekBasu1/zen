use std::sync::Arc;

use fs::Fs;
use gpui::{AnyElement, App, Context, DismissEvent, SharedString, Task, Window};
use picker::{Picker, PickerDelegate};
use settings::{Settings, update_settings_file};
use theme::{Appearance, SystemAppearance, ThemeRegistry};
use theme_settings::ThemeSettings;
use ui::{ListItem, ListItemSpacing, prelude::*};

type ThemeSelector = Picker<ThemeSelectorDelegate>;

#[derive(Clone)]
struct ThemeOption {
    name: SharedString,
    appearance: Appearance,
}

pub struct ThemeSelectorDelegate {
    themes: Vec<ThemeOption>,
    filtered_indices: Vec<usize>,
    selected_index: usize,
    current_theme: SharedString,
    fs: Arc<dyn Fs>,
}

impl ThemeSelectorDelegate {
    fn new(fs: Arc<dyn Fs>, cx: &mut Context<ThemeSelector>) -> Self {
        let registry = ThemeRegistry::global(cx);
        let mut themes = registry
            .list()
            .into_iter()
            .map(|theme| ThemeOption {
                name: theme.name,
                appearance: theme.appearance,
            })
            .collect::<Vec<_>>();
        themes.sort_by(|left, right| left.name.cmp(&right.name));

        let current_theme: SharedString = ThemeSettings::get_global(cx)
            .theme
            .name(SystemAppearance::global(cx).0)
            .0
            .to_string()
            .into();
        let filtered_indices = (0..themes.len()).collect::<Vec<_>>();
        let selected_index = filtered_indices
            .iter()
            .position(|index| themes[*index].name == current_theme)
            .unwrap_or(0);

        Self {
            themes,
            filtered_indices,
            selected_index,
            current_theme,
            fs,
        }
    }

    fn selected_theme(&self) -> Option<&ThemeOption> {
        self.filtered_indices
            .get(self.selected_index)
            .and_then(|index| self.themes.get(*index))
    }
}

impl PickerDelegate for ThemeSelectorDelegate {
    type ListItem = AnyElement;

    fn match_count(&self) -> usize {
        self.filtered_indices.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(
        &mut self,
        index: usize,
        _: &mut Window,
        cx: &mut Context<ThemeSelector>,
    ) {
        self.selected_index = index.min(self.filtered_indices.len().saturating_sub(1));
        cx.notify();
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Search themes...".into()
    }

    fn no_matches_text(&self, _window: &mut Window, _cx: &mut App) -> Option<SharedString> {
        Some("No themes found".into())
    }

    fn update_matches(
        &mut self,
        query: String,
        _window: &mut Window,
        cx: &mut Context<ThemeSelector>,
    ) -> Task<()> {
        let query = query.trim().to_lowercase();
        self.filtered_indices = self
            .themes
            .iter()
            .enumerate()
            .filter_map(|(index, theme)| {
                if query.is_empty() || theme.name.to_lowercase().contains(&query) {
                    Some(index)
                } else {
                    None
                }
            })
            .collect();

        self.selected_index = self
            .filtered_indices
            .iter()
            .position(|index| self.themes[*index].name == self.current_theme)
            .unwrap_or(0)
            .min(self.filtered_indices.len().saturating_sub(1));
        cx.notify();

        Task::ready(())
    }

    fn confirm(&mut self, _secondary: bool, _window: &mut Window, cx: &mut Context<ThemeSelector>) {
        let Some(theme) = self.selected_theme().cloned() else {
            cx.emit(DismissEvent);
            return;
        };

        let fs = self.fs.clone();
        let theme_name = theme.name.to_string();
        let theme_appearance = theme.appearance;
        update_settings_file(fs, cx, move |settings, cx| {
            theme_settings::set_theme(
                settings,
                theme_name,
                theme_appearance,
                SystemAppearance::global(cx).0,
            );
        });
        cx.emit(DismissEvent);
    }

    fn dismissed(&mut self, _window: &mut Window, cx: &mut Context<ThemeSelector>) {
        cx.emit(DismissEvent);
    }

    fn render_match(
        &self,
        index: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<ThemeSelector>,
    ) -> Option<Self::ListItem> {
        let theme = self
            .filtered_indices
            .get(index)
            .and_then(|theme_index| self.themes.get(*theme_index))?;
        let appearance = match theme.appearance {
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
        };

        Some(
            ListItem::new(index)
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .toggle_state(selected)
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .gap_3()
                        .child(Label::new(theme.name.clone()))
                        .child(
                            Label::new(appearance)
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        ),
                )
                .into_any_element(),
        )
    }
}

pub fn theme_selector(
    fs: Arc<dyn Fs>,
    window: &mut Window,
    cx: &mut Context<ThemeSelector>,
) -> ThemeSelector {
    let delegate = ThemeSelectorDelegate::new(fs, cx);

    Picker::uniform_list(delegate, window, cx)
        .show_scrollbar(true)
        .width(rems_from_px(320.))
        .max_height(Some(rems(24.).into()))
}
