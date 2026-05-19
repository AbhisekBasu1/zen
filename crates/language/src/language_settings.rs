//! Provides `language`-related settings.

use crate::{Buffer, BufferSnapshot, File, LanguageName};
use collections::{FxHashMap, HashMap};
use ec4rs::{
    Properties as EditorconfigProperties,
    property::{
        EndOfLine, FinalNewline, IndentSize, IndentStyle, MaxLineLen, TabWidth, TrimTrailingWs,
    },
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use gpui::{App, SharedString};

pub use settings::{
    AutoIndentMode, LanguageSettingsContent, LineEndingSetting, RewrapBehavior,
    ShowWhitespaceSetting, SoftWrap,
};
use settings::{RegisterSetting, Settings, SettingsLocation, SettingsStore, merge_from::MergeFrom};
use std::{borrow::Cow, num::NonZeroU32, sync::Arc};
use text::ToOffset;

/// Returns the settings for all languages from the provided file.
pub fn all_language_settings<'a>(
    file: Option<&'a Arc<dyn File>>,
    cx: &'a App,
) -> &'a AllLanguageSettings {
    let location = file.map(|f| SettingsLocation {
        worktree_id: f.worktree_id(cx),
        path: f.path().as_ref(),
    });
    AllLanguageSettings::get(location, cx)
}

/// The settings for all languages.
#[derive(Debug, Clone, RegisterSetting)]
pub struct AllLanguageSettings {
    pub defaults: LanguageSettings,
    languages: HashMap<LanguageName, LanguageSettings>,
    pub file_types: FxHashMap<Arc<str>, (GlobSet, Vec<String>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WhitespaceMap {
    pub space: SharedString,
    pub tab: SharedString,
}

/// The settings for a particular language.
#[derive(Debug, Clone, PartialEq)]
pub struct LanguageSettings {
    /// How many columns a tab should occupy.
    pub tab_size: NonZeroU32,
    /// Whether to indent lines using tab characters, as opposed to multiple
    /// spaces.
    pub hard_tabs: bool,
    /// How to soft-wrap long lines of text.
    pub soft_wrap: settings::SoftWrap,
    /// The column at which to soft-wrap lines, for buffers where soft-wrap
    /// is enabled.
    pub preferred_line_length: u32,
    /// Whether to show wrap guides (vertical rulers) in the editor.
    /// Setting this to true will show a guide at the 'preferred_line_length' value
    /// if softwrap is set to 'preferred_line_length', and will show any
    /// additional guides as specified by the 'wrap_guides' setting.
    pub show_wrap_guides: bool,
    /// Character counts at which to show wrap guides (vertical rulers) in the editor.
    pub wrap_guides: Vec<usize>,
    /// Indent guide related settings.
    pub indent_guides: IndentGuideSettings,
    /// Whether or not to remove any trailing whitespace from lines of a buffer
    /// before saving it.
    pub remove_trailing_whitespace_on_save: bool,
    /// Whether or not to ensure there's a single newline at the end of a buffer
    /// when saving it.
    pub ensure_final_newline_on_save: bool,
    /// How line endings are initialized for new files and normalized during
    /// format and save.
    pub line_ending: LineEndingSetting,
    /// Controls where the `editor::Rewrap` action is allowed for this language.
    pub allow_rewrap: RewrapBehavior,
    /// Whether to show tabs and spaces in the editor.
    pub show_whitespaces: settings::ShowWhitespaceSetting,
    /// Visible characters used to render whitespace when show_whitespaces is enabled.
    pub whitespace_map: WhitespaceMap,
    /// Whether to start a new line with a comment when a previous line is a comment as well.
    pub extend_comment_on_newline: bool,
    /// Whether to continue markdown lists when pressing enter.
    pub extend_list_on_newline: bool,
    /// Whether to indent list items when pressing tab after a list marker.
    pub indent_list_on_tab: bool,
    /// Whether to automatically close brackets.
    pub use_autoclose: bool,
    /// Whether to automatically surround text with brackets.
    pub use_auto_surround: bool,
    /// Controls automatic indentation behavior when typing.
    pub auto_indent: AutoIndentMode,
    /// Whether indentation of pasted content should be adjusted based on the context.
    pub auto_indent_on_paste: bool,
    /// Controls how the editor handles the autoclosed characters.
    pub always_treat_brackets_as_autoclosed: bool,
    /// Whether to enable word diff highlighting in the editor.
    ///
    /// When enabled, changed words within modified lines are highlighted
    /// to show exactly what changed.
    ///
    /// Default: `true`
    pub word_diff_enabled: bool,
    /// Whether to use tree-sitter bracket queries to detect and colorize the brackets in the editor.
    pub colorize_brackets: bool,
}

/// The settings for indent guides.
#[derive(Debug, Clone, PartialEq)]
pub struct IndentGuideSettings {
    /// Whether to display indent guides in the editor.
    ///
    /// Default: true
    pub enabled: bool,
    /// The width of the indent guides in pixels, between 1 and 10.
    ///
    /// Default: 1
    pub line_width: u32,
    /// The width of the active indent guide in pixels, between 1 and 10.
    ///
    /// Default: 1
    pub active_line_width: u32,
    /// Determines how indent guides are colored.
    ///
    /// Default: Fixed
    pub coloring: settings::IndentGuideColoring,
    /// Determines how indent guide backgrounds are colored.
    ///
    /// Default: Disabled
    pub background_coloring: settings::IndentGuideBackgroundColoring,
}

impl IndentGuideSettings {
    /// Returns the clamped line width in pixels for an indent guide based on
    /// whether it is active, or `None` when line coloring is disabled.
    pub fn visible_line_width(&self, active: bool) -> Option<u32> {
        if self.coloring == settings::IndentGuideColoring::Disabled {
            return None;
        }
        let width = if active {
            self.active_line_width
        } else {
            self.line_width
        };
        Some(width.clamp(1, 10))
    }
}

impl LanguageSettings {
    pub fn for_buffer<'a>(buffer: &'a Buffer, cx: &'a App) -> Cow<'a, LanguageSettings> {
        Self::resolve(Some(buffer), None, cx)
    }

    pub fn for_buffer_at<'a, D: ToOffset>(
        buffer: &'a Buffer,
        position: D,
        cx: &'a App,
    ) -> Cow<'a, LanguageSettings> {
        let language = buffer.language_at(position);
        Self::resolve(Some(buffer), language.map(|l| l.name()).as_ref(), cx)
    }

    pub fn for_buffer_snapshot<'a>(
        buffer: &'a BufferSnapshot,
        offset: Option<usize>,
        cx: &'a App,
    ) -> Cow<'a, LanguageSettings> {
        let location = buffer.file().map(|f| SettingsLocation {
            worktree_id: f.worktree_id(cx),
            path: f.path().as_ref(),
        });

        let language = if let Some(offset) = offset {
            buffer.language_at(offset)
        } else {
            buffer.language()
        };

        AllLanguageSettings::get(location, cx).language(
            location,
            language.map(|l| l.name()).as_ref(),
            cx,
        )
    }

    pub fn resolve<'a>(
        buffer: Option<&'a Buffer>,
        override_language: Option<&LanguageName>,
        cx: &'a App,
    ) -> Cow<'a, LanguageSettings> {
        let Some(buffer) = buffer else {
            return AllLanguageSettings::get(None, cx).language(None, override_language, cx);
        };
        let location = buffer.file().map(|f| SettingsLocation {
            worktree_id: f.worktree_id(cx),
            path: f.path().as_ref(),
        });
        let all = AllLanguageSettings::get(location, cx);
        if override_language.is_none() {
            all.language(location, buffer.language().map(|l| l.name()).as_ref(), cx)
        } else {
            all.language(location, override_language, cx)
        }
    }
}

impl AllLanguageSettings {
    /// Returns the [`LanguageSettings`] for the language with the specified name.
    pub fn language<'a>(
        &'a self,
        location: Option<SettingsLocation<'a>>,
        language_name: Option<&LanguageName>,
        cx: &'a App,
    ) -> Cow<'a, LanguageSettings> {
        let settings = language_name
            .and_then(|name| self.languages.get(name))
            .unwrap_or(&self.defaults);

        let editorconfig_properties = location.and_then(|location| {
            cx.global::<SettingsStore>()
                .editorconfig_store
                .read(cx)
                .properties(location.worktree_id, location.path)
        });
        if let Some(editorconfig_properties) = editorconfig_properties {
            let mut settings = settings.clone();
            merge_with_editorconfig(&mut settings, &editorconfig_properties);
            Cow::Owned(settings)
        } else {
            Cow::Borrowed(settings)
        }
    }
}

fn merge_with_editorconfig(settings: &mut LanguageSettings, cfg: &EditorconfigProperties) {
    let preferred_line_length = cfg.get::<MaxLineLen>().ok().and_then(|v| match v {
        MaxLineLen::Value(u) => Some(u as u32),
        MaxLineLen::Off => None,
    });
    let tab_size = cfg.get::<IndentSize>().ok().and_then(|v| match v {
        IndentSize::Value(u) => NonZeroU32::new(u as u32),
        IndentSize::UseTabWidth => cfg.get::<TabWidth>().ok().and_then(|w| match w {
            TabWidth::Value(u) => NonZeroU32::new(u as u32),
        }),
    });
    let hard_tabs = cfg
        .get::<IndentStyle>()
        .map(|v| v.eq(&IndentStyle::Tabs))
        .ok();
    let ensure_final_newline_on_save = cfg
        .get::<FinalNewline>()
        .map(|v| match v {
            FinalNewline::Value(b) => b,
        })
        .ok();
    let remove_trailing_whitespace_on_save = cfg
        .get::<TrimTrailingWs>()
        .map(|v| match v {
            TrimTrailingWs::Value(b) => b,
        })
        .ok();
    let line_ending = cfg.get::<EndOfLine>().ok().and_then(|v| match v {
        EndOfLine::Lf => Some(LineEndingSetting::EnforceLf),
        EndOfLine::CrLf => Some(LineEndingSetting::EnforceCrlf),
        EndOfLine::Cr => None,
    });

    settings
        .preferred_line_length
        .merge_from_option(preferred_line_length.as_ref());
    settings.tab_size.merge_from_option(tab_size.as_ref());
    settings.hard_tabs.merge_from_option(hard_tabs.as_ref());
    settings
        .remove_trailing_whitespace_on_save
        .merge_from_option(remove_trailing_whitespace_on_save.as_ref());
    settings
        .ensure_final_newline_on_save
        .merge_from_option(ensure_final_newline_on_save.as_ref());
    settings.line_ending.merge_from_option(line_ending.as_ref());
}

impl settings::Settings for AllLanguageSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let all_languages = &content.project.all_languages;

        fn load_from_content(settings: LanguageSettingsContent) -> LanguageSettings {
            let indent_guides = settings.indent_guides.unwrap();
            let whitespace_map = settings.whitespace_map.unwrap();

            LanguageSettings {
                tab_size: settings.tab_size.unwrap(),
                hard_tabs: settings.hard_tabs.unwrap(),
                soft_wrap: settings.soft_wrap.unwrap(),
                preferred_line_length: settings.preferred_line_length.unwrap(),
                show_wrap_guides: settings.show_wrap_guides.unwrap(),
                wrap_guides: settings.wrap_guides.unwrap(),
                indent_guides: IndentGuideSettings {
                    enabled: indent_guides.enabled.unwrap(),
                    line_width: indent_guides.line_width.unwrap(),
                    active_line_width: indent_guides.active_line_width.unwrap(),
                    coloring: indent_guides.coloring.unwrap(),
                    background_coloring: indent_guides.background_coloring.unwrap(),
                },
                remove_trailing_whitespace_on_save: settings
                    .remove_trailing_whitespace_on_save
                    .unwrap(),
                ensure_final_newline_on_save: settings.ensure_final_newline_on_save.unwrap(),
                line_ending: settings.line_ending.unwrap(),
                allow_rewrap: settings.allow_rewrap.unwrap(),
                show_whitespaces: settings.show_whitespaces.unwrap(),
                whitespace_map: WhitespaceMap {
                    space: SharedString::new(whitespace_map.space.unwrap().to_string()),
                    tab: SharedString::new(whitespace_map.tab.unwrap().to_string()),
                },
                extend_comment_on_newline: settings.extend_comment_on_newline.unwrap(),
                extend_list_on_newline: settings.extend_list_on_newline.unwrap(),
                indent_list_on_tab: settings.indent_list_on_tab.unwrap(),
                use_autoclose: settings.use_autoclose.unwrap(),
                use_auto_surround: settings.use_auto_surround.unwrap(),
                auto_indent: settings.auto_indent.unwrap(),
                auto_indent_on_paste: settings.auto_indent_on_paste.unwrap(),
                always_treat_brackets_as_autoclosed: settings
                    .always_treat_brackets_as_autoclosed
                    .unwrap(),
                colorize_brackets: settings.colorize_brackets.unwrap(),
                word_diff_enabled: settings.word_diff_enabled.unwrap(),
            }
        }

        let default_language_settings = load_from_content(all_languages.defaults.clone());

        let mut languages = HashMap::default();
        for (language_name, settings) in &all_languages.languages.0 {
            let mut language_settings = all_languages.defaults.clone();
            settings::merge_from::MergeFrom::merge_from(&mut language_settings, settings);
            languages.insert(
                LanguageName(language_name.clone().into()),
                load_from_content(language_settings),
            );
        }

        let mut file_types: FxHashMap<Arc<str>, (GlobSet, Vec<String>)> = FxHashMap::default();

        for (language, patterns) in all_languages.file_types.iter().flatten() {
            let mut builder = GlobSetBuilder::new();

            for pattern in &patterns.0 {
                builder.add(Glob::new(pattern).unwrap());
            }

            file_types.insert(
                language.clone(),
                (builder.build().unwrap(), patterns.0.clone()),
            );
        }

        Self {
            defaults: default_language_settings,
            languages,
            file_types,
        }
    }
}
