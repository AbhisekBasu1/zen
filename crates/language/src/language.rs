//! The `language` crate provides a large chunk of Zed's language-related
//! features.
//! Namely, this crate:
//! - Provides [`Language`], [`Grammar`] and [`LanguageRegistry`] types that
//!   use Tree-sitter to provide syntax highlighting to the editor; note though that `language` doesn't perform the highlighting by itself. It only maps ranges in a buffer to colors. Treesitter is also used for buffer outlines (lists of symbols in a buffer)
//! - Exposes [`LanguageConfig`] that describes how constructs (like brackets or line comments) should be handled by the editor for a source file of a particular language.
//!
//! Notably we do *not* assign a single language to a single file; in real world a single file can consist of multiple programming languages - HTML is a good example of that - and `language` crate tends to reflect that status quo in its API.
mod buffer;
mod file_content;
mod language_registry;

pub mod language_settings;
mod outline;
mod syntax_map;
mod text_diff;

pub use crate::language_settings::{AutoIndentMode, IndentGuideSettings};
use anyhow::Result;
use collections::HashSet;
use gpui::Entity;

pub use language_core::highlight_map::{HighlightId, HighlightMap};

pub use language_core::{
    BlockCommentConfig, BracketPair, BracketPairConfig, BracketPairContent, BracketsConfig,
    BracketsPatternConfig, CodeLabel, CodeLabelBuilder, DecreaseIndentConfig, Grammar, GrammarId,
    HighlightsConfig, IndentConfig, InjectionConfig, InjectionPatternConfig, LanguageConfig,
    LanguageConfigOverride, LanguageId, LanguageMatcher, OrderedListConfig, OutlineConfig,
    Override, OverrideConfig, OverrideEntry, RedactionConfig, SoftWrap, TaskListConfig, TextObject,
    TextObjectConfig, WrapCharactersConfig, auto_indent_using_last_non_empty_line_default,
    deserialize_regex, deserialize_regex_vec, regex_json_schema, regex_vec_json_schema,
    serialize_regex,
};
pub use language_registry::{LanguageName, LoadedLanguage};
use parking_lot::Mutex;
use regex::Regex;
use std::{
    fmt::Debug,
    hash::Hash,
    ops::{DerefMut, Range},
    str,
    sync::{Arc, LazyLock},
};
use syntax_map::{QueryCursorHandle, SyntaxSnapshot};
pub use text_diff::{
    DiffOptions, apply_diff_patch, apply_reversed_diff_patch, char_diff, line_diff, text_diff,
    text_diff_with_options, unified_diff, unified_diff_with_context, unified_diff_with_offsets,
    word_diff_ranges,
};
use theme::SyntaxTheme;
use tree_sitter::{self, QueryCursor};
#[cfg(feature = "wasm-grammars")]
use tree_sitter::{WasmStore, wasmtime};

pub use buffer::Operation;
pub use buffer::*;
pub use file_content::{ByteContent, FILE_ANALYSIS_BYTES, analyze_byte_content};
pub use language_registry::{
    AvailableLanguage, LanguageNotFound, LanguageQueries, LanguageRegistry, QUERY_FILENAME_PREFIXES,
};
pub use outline::*;
pub use syntax_map::{
    OwnedSyntaxLayer, SyntaxLayer, SyntaxMapMatches, ToTreeSitterPoint, TreeSitterOptions,
};
pub use text::{AnchorRangeExt, LineEnding};
pub use tree_sitter::{Node, Parser, Tree, TreeCursor};

pub(crate) fn to_settings_soft_wrap(value: language_core::SoftWrap) -> settings::SoftWrap {
    match value {
        language_core::SoftWrap::None => settings::SoftWrap::None,
        language_core::SoftWrap::PreferLine => settings::SoftWrap::PreferLine,
        language_core::SoftWrap::EditorWidth => settings::SoftWrap::EditorWidth,
        language_core::SoftWrap::Bounded => settings::SoftWrap::Bounded,
    }
}

static QUERY_CURSORS: Mutex<Vec<QueryCursor>> = Mutex::new(vec![]);
static PARSERS: Mutex<Vec<Parser>> = Mutex::new(vec![]);

#[ztracing::instrument(skip_all)]
pub fn with_parser<F, R>(func: F) -> R
where
    F: FnOnce(&mut Parser) -> R,
{
    let mut parser = PARSERS.lock().pop().unwrap_or_else(|| {
        #[cfg(feature = "wasm-grammars")]
        let mut parser = Parser::new();
        #[cfg(not(feature = "wasm-grammars"))]
        let parser = Parser::new();
        #[cfg(feature = "wasm-grammars")]
        {
            parser
                .set_wasm_store(WasmStore::new(&WASM_ENGINE).unwrap())
                .unwrap();
        }
        parser
    });
    parser.set_included_ranges(&[]).unwrap();
    let result = func(&mut parser);
    PARSERS.lock().push(parser);
    result
}

pub fn with_query_cursor<F, R>(func: F) -> R
where
    F: FnOnce(&mut QueryCursor) -> R,
{
    let mut cursor = QueryCursorHandle::new();
    func(cursor.deref_mut())
}

#[cfg(feature = "wasm-grammars")]
static WASM_ENGINE: LazyLock<wasmtime::Engine> = LazyLock::new(|| {
    wasmtime::Engine::new(&wasmtime::Config::new()).expect("Failed to create Wasmtime engine")
});

/// A shared grammar for plain text, exposed for reuse by downstream crates.
pub static PLAIN_TEXT: LazyLock<Arc<Language>> = LazyLock::new(|| {
    Arc::new(Language::new(
        LanguageConfig {
            name: "Plain Text".into(),
            soft_wrap: Some(SoftWrap::EditorWidth),
            matcher: LanguageMatcher {
                path_suffixes: vec!["txt".to_owned()],
                first_line_pattern: None,
            },
            brackets: BracketPairConfig {
                pairs: vec![
                    BracketPair {
                        start: "(".to_string(),
                        end: ")".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "[".to_string(),
                        end: "]".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "{".to_string(),
                        end: "}".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "\"".to_string(),
                        end: "\"".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "'".to_string(),
                        end: "'".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                ],
                disabled_scopes_by_bracket_ix: Default::default(),
            },
            ..Default::default()
        },
        None,
    ))
});

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Location {
    pub buffer: Entity<Buffer>,
    pub range: Range<Anchor>,
}

/// Represents a language for the given range. Some languages (e.g. HTML)
/// interleave several languages together, thus a single buffer might actually contain
/// several nested scopes.
#[derive(Clone, Debug)]
pub struct LanguageScope {
    language: Arc<Language>,
    override_id: Option<u32>,
}

pub struct Language {
    pub(crate) id: LanguageId,
    pub(crate) config: LanguageConfig,
    pub(crate) grammar: Option<Arc<Grammar>>,
}

impl Language {
    pub fn new(config: LanguageConfig, ts_language: Option<tree_sitter::Language>) -> Self {
        Self::new_with_id(LanguageId::new(), config, ts_language)
    }

    pub fn id(&self) -> LanguageId {
        self.id
    }

    fn new_with_id(
        id: LanguageId,
        config: LanguageConfig,
        ts_language: Option<tree_sitter::Language>,
    ) -> Self {
        Self {
            id,
            config,
            grammar: ts_language.map(|ts_language| Arc::new(Grammar::new(ts_language))),
        }
    }

    pub fn with_queries(mut self, queries: LanguageQueries) -> Result<Self> {
        if let Some(grammar) = self.grammar.take() {
            let grammar =
                Arc::try_unwrap(grammar).map_err(|_| anyhow::anyhow!("cannot mutate grammar"))?;
            let grammar = grammar.with_queries(queries, &mut self.config)?;
            self.grammar = Some(Arc::new(grammar));
        }
        Ok(self)
    }

    pub fn with_highlights_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query(|grammar| grammar.with_highlights_query(source))
    }

    pub fn with_outline_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_outline_query(source, name))
    }

    pub fn with_text_object_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| {
            grammar.with_text_object_query(source, name)
        })
    }

    pub fn with_brackets_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_brackets_query(source, name))
    }

    pub fn with_indents_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_indents_query(source, name))
    }

    pub fn with_injection_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_injection_query(source, name))
    }

    pub fn with_override_query(mut self, source: &str) -> Result<Self> {
        if let Some(grammar_arc) = self.grammar.take() {
            let grammar = Arc::try_unwrap(grammar_arc)
                .map_err(|_| anyhow::anyhow!("cannot mutate grammar"))?;
            let grammar = grammar.with_override_query(
                source,
                &self.config.name,
                &self.config.overrides,
                &mut self.config.brackets,
            )?;
            self.grammar = Some(Arc::new(grammar));
        }
        Ok(self)
    }

    pub fn with_redaction_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_redaction_query(source, name))
    }

    fn with_grammar_query(
        mut self,
        build: impl FnOnce(Grammar) -> Result<Grammar>,
    ) -> Result<Self> {
        if let Some(grammar_arc) = self.grammar.take() {
            let grammar = Arc::try_unwrap(grammar_arc)
                .map_err(|_| anyhow::anyhow!("cannot mutate grammar"))?;
            self.grammar = Some(Arc::new(build(grammar)?));
        }
        Ok(self)
    }

    fn with_grammar_query_and_name(
        mut self,
        build: impl FnOnce(Grammar, &LanguageName) -> Result<Grammar>,
    ) -> Result<Self> {
        if let Some(grammar_arc) = self.grammar.take() {
            let grammar = Arc::try_unwrap(grammar_arc)
                .map_err(|_| anyhow::anyhow!("cannot mutate grammar"))?;
            self.grammar = Some(Arc::new(build(grammar, &self.config.name)?));
        }
        Ok(self)
    }

    pub fn name(&self) -> LanguageName {
        self.config.name.clone()
    }

    pub fn code_fence_block_name(&self) -> Arc<str> {
        self.config
            .code_fence_block_name
            .clone()
            .unwrap_or_else(|| self.config.name.as_ref().to_lowercase().into())
    }

    pub fn highlight_text<'a>(
        self: &'a Arc<Self>,
        text: &'a Rope,
        range: Range<usize>,
    ) -> Vec<(Range<usize>, HighlightId)> {
        let mut result = Vec::new();
        if let Some(grammar) = &self.grammar {
            let tree = parse_text(grammar, text, None);
            let captures =
                SyntaxSnapshot::single_tree_captures(range.clone(), text, &tree, self, |grammar| {
                    grammar
                        .highlights_config
                        .as_ref()
                        .map(|config| &config.query)
                });
            let highlight_maps = vec![grammar.highlight_map()];
            let mut offset = 0;
            for chunk in BufferChunks::new(text, range, Some((captures, highlight_maps)), None) {
                let end_offset = offset + chunk.text.len();
                if let Some(highlight_id) = chunk.syntax_highlight_id {
                    result.push((offset..end_offset, highlight_id));
                }
                offset = end_offset;
            }
        }
        result
    }

    pub fn path_suffixes(&self) -> &[String] {
        &self.config.matcher.path_suffixes
    }

    pub fn should_autoclose_before(&self, c: char) -> bool {
        c.is_whitespace() || self.config.autoclose_before.contains(c)
    }

    pub fn set_theme(&self, theme: &SyntaxTheme) {
        if let Some(grammar) = self.grammar.as_ref()
            && let Some(highlights_config) = &grammar.highlights_config
        {
            *grammar.highlight_map.lock() =
                build_highlight_map(highlights_config.query.capture_names(), theme);
        }
    }

    pub fn grammar(&self) -> Option<&Arc<Grammar>> {
        self.grammar.as_ref()
    }

    pub fn default_scope(self: &Arc<Self>) -> LanguageScope {
        LanguageScope {
            language: self.clone(),
            override_id: None,
        }
    }

    pub fn config(&self) -> &LanguageConfig {
        &self.config
    }
}

#[inline]
pub fn build_highlight_map(capture_names: &[&str], theme: &SyntaxTheme) -> HighlightMap {
    HighlightMap::from_ids(
        capture_names
            .iter()
            .map(|capture_name| theme.highlight_id(capture_name).map(HighlightId::new)),
    )
}

impl LanguageScope {
    pub fn path_suffixes(&self) -> &[String] {
        self.language.path_suffixes()
    }

    pub fn language_name(&self) -> LanguageName {
        self.language.config.name.clone()
    }

    pub fn collapsed_placeholder(&self) -> &str {
        self.language.config.collapsed_placeholder.as_ref()
    }

    /// Returns line prefix that is inserted in e.g. line continuations or
    /// in `toggle comments` action.
    pub fn line_comment_prefixes(&self) -> &[Arc<str>] {
        Override::as_option(
            self.config_override().map(|o| &o.line_comments),
            Some(&self.language.config.line_comments),
        )
        .map_or([].as_slice(), |e| e.as_slice())
    }

    /// Config for block comments for this language.
    pub fn block_comment(&self) -> Option<&BlockCommentConfig> {
        Override::as_option(
            self.config_override().map(|o| &o.block_comment),
            self.language.config.block_comment.as_ref(),
        )
    }

    /// Config for documentation-style block comments for this language.
    pub fn documentation_comment(&self) -> Option<&BlockCommentConfig> {
        self.language.config.documentation_comment.as_ref()
    }

    /// Returns list markers that are inserted unchanged on newline (e.g., `- `, `* `, `+ `).
    pub fn unordered_list(&self) -> &[Arc<str>] {
        &self.language.config.unordered_list
    }

    /// Returns configuration for ordered lists with auto-incrementing numbers (e.g., `1. ` becomes `2. `).
    pub fn ordered_list(&self) -> &[OrderedListConfig] {
        &self.language.config.ordered_list
    }

    /// Returns configuration for task list continuation, if any (e.g., `- [x] ` continues as `- [ ] `).
    pub fn task_list(&self) -> Option<&TaskListConfig> {
        self.language.config.task_list.as_ref()
    }

    /// Returns additional regex patterns that act as prefix markers for creating
    /// boundaries during rewrapping.
    ///
    /// By default, Zed treats as paragraph and comment prefixes as boundaries.
    pub fn rewrap_prefixes(&self) -> &[Regex] {
        &self.language.config.rewrap_prefixes
    }

    /// Returns a list of language-specific word characters.
    ///
    /// By default, Zed treats alphanumeric characters (and '_') as word characters for
    /// the purpose of actions like 'move to next word end` or whole-word search.
    /// It additionally accounts for language's additional word characters.
    pub fn word_characters(&self) -> Option<&HashSet<char>> {
        Override::as_option(
            self.config_override().map(|o| &o.word_characters),
            Some(&self.language.config.word_characters),
        )
    }

    /// Returns a list of bracket pairs for a given language with an additional
    /// piece of information about whether the particular bracket pair is currently active for a given language.
    pub fn brackets(&self) -> impl Iterator<Item = (&BracketPair, bool)> {
        let mut disabled_ids = self
            .config_override()
            .map_or(&[] as _, |o| o.disabled_bracket_ixs.as_slice());
        self.language
            .config
            .brackets
            .pairs
            .iter()
            .enumerate()
            .map(move |(ix, bracket)| {
                let mut is_enabled = true;
                if let Some(next_disabled_ix) = disabled_ids.first()
                    && ix == *next_disabled_ix as usize
                {
                    disabled_ids = &disabled_ids[1..];
                    is_enabled = false;
                }
                (bracket, is_enabled)
            })
    }

    pub fn should_autoclose_before(&self, c: char) -> bool {
        c.is_whitespace() || self.language.config.autoclose_before.contains(c)
    }

    pub fn override_name(&self) -> Option<&str> {
        let id = self.override_id?;
        let grammar = self.language.grammar.as_ref()?;
        let override_config = grammar.override_config.as_ref()?;
        override_config.values.get(&id).map(|e| e.name.as_str())
    }

    fn config_override(&self) -> Option<&LanguageConfigOverride> {
        let id = self.override_id?;
        let grammar = self.language.grammar.as_ref()?;
        let override_config = grammar.override_config.as_ref()?;
        override_config.values.get(&id).map(|e| &e.value)
    }
}

impl Hash for Language {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl Eq for Language {}

impl Debug for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Language")
            .field("name", &self.config.name)
            .finish()
    }
}

pub(crate) fn parse_text(grammar: &Grammar, text: &Rope, old_tree: Option<Tree>) -> Tree {
    with_parser(|parser| {
        parser
            .set_language(&grammar.ts_language)
            .expect("incompatible grammar");
        let mut chunks = text.chunks_in_range(0..text.len());
        parser
            .parse_with_options(
                &mut move |offset, _| {
                    chunks.seek(offset);
                    chunks.next().unwrap_or("").as_bytes()
                },
                old_tree.as_ref(),
                None,
            )
            .unwrap()
    })
}

#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub fn rust_lang() -> Arc<Language> {
    let language = Language::new(
        LanguageConfig {
            name: "Rust".into(),
            matcher: LanguageMatcher {
                path_suffixes: vec!["rs".to_string()],
                ..Default::default()
            },
            line_comments: vec!["// ".into(), "/// ".into(), "//! ".into()],
            brackets: BracketPairConfig {
                pairs: vec![
                    BracketPair {
                        start: "{".into(),
                        end: "}".into(),
                        close: true,
                        surround: false,
                        newline: true,
                    },
                    BracketPair {
                        start: "[".into(),
                        end: "]".into(),
                        close: true,
                        surround: false,
                        newline: true,
                    },
                    BracketPair {
                        start: "(".into(),
                        end: ")".into(),
                        close: true,
                        surround: false,
                        newline: true,
                    },
                    BracketPair {
                        start: "<".into(),
                        end: ">".into(),
                        close: false,
                        surround: false,
                        newline: true,
                    },
                    BracketPair {
                        start: "\"".into(),
                        end: "\"".into(),
                        close: true,
                        surround: false,
                        newline: false,
                    },
                ],
                ..Default::default()
            },
            ..Default::default()
        },
        None,
    );
    Arc::new(language)
}

#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub fn markdown_lang() -> Arc<Language> {
    use std::borrow::Cow;

    let language = Language::new(
        LanguageConfig {
            name: "Markdown".into(),
            matcher: LanguageMatcher {
                path_suffixes: vec!["md".into()],
                ..Default::default()
            },
            ..LanguageConfig::default()
        },
        Some(tree_sitter_md::LANGUAGE.into()),
    )
    .with_queries(LanguageQueries {
        brackets: Some(Cow::from(include_str!(
            "../../grammars/src/markdown/brackets.scm"
        ))),
        injections: Some(Cow::from(include_str!(
            "../../grammars/src/markdown/injections.scm"
        ))),
        highlights: Some(Cow::from(include_str!(
            "../../grammars/src/markdown/highlights.scm"
        ))),
        indents: Some(Cow::from(include_str!(
            "../../grammars/src/markdown/indents.scm"
        ))),
        outline: Some(Cow::from(include_str!(
            "../../grammars/src/markdown/outline.scm"
        ))),
        ..LanguageQueries::default()
    })
    .expect("Could not parse markdown queries");
    Arc::new(language)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, rgba};
    use pretty_assertions::assert_matches;

    #[test]
    fn test_highlight_map() {
        let theme = SyntaxTheme::new(
            [
                ("function", rgba(0x100000ff)),
                ("function.method", rgba(0x200000ff)),
                ("function.async", rgba(0x300000ff)),
                ("variable.builtin.self.rust", rgba(0x400000ff)),
                ("variable.builtin", rgba(0x500000ff)),
                ("variable", rgba(0x600000ff)),
            ]
            .iter()
            .map(|(name, color)| (name.to_string(), (*color).into())),
        );

        let capture_names = &[
            "function.special",
            "function.async.rust",
            "variable.builtin.self",
        ];

        let map = build_highlight_map(capture_names, &theme);
        assert_eq!(
            theme.get_capture_name(map.get(0).unwrap()),
            Some("function")
        );
        assert_eq!(
            theme.get_capture_name(map.get(1).unwrap()),
            Some("function.async")
        );
        assert_eq!(
            theme.get_capture_name(map.get(2).unwrap()),
            Some("variable.builtin")
        );
    }

    #[gpui::test(iterations = 10)]

    async fn test_language_loading(cx: &mut TestAppContext) {
        let languages = LanguageRegistry::test(cx.executor());
        let languages = Arc::new(languages);
        languages.register_native_grammars([("markdown", tree_sitter_md::LANGUAGE)]);
        languages.register_test_language(LanguageConfig {
            name: "Markdown".into(),
            grammar: Some("markdown".into()),
            matcher: LanguageMatcher {
                path_suffixes: vec!["md".into()],
                ..Default::default()
            },
            ..Default::default()
        });
        assert_eq!(
            languages.language_names(),
            &[
                LanguageName::new_static("Markdown"),
                LanguageName::new_static("Plain Text"),
            ]
        );

        let markdown1 = languages.language_for_name("Markdown");
        let markdown2 = languages.language_for_name("Markdown");

        // Ensure language is still listed even if it's being loaded.
        assert_eq!(
            languages.language_names(),
            &[
                LanguageName::new_static("Markdown"),
                LanguageName::new_static("Plain Text"),
            ]
        );

        let (markdown1, markdown2) = futures::join!(markdown1, markdown2);
        assert!(Arc::ptr_eq(&markdown1.unwrap(), &markdown2.unwrap()));

        // Ensure language is still listed even after loading it.
        assert_eq!(
            languages.language_names(),
            &[
                LanguageName::new_static("Markdown"),
                LanguageName::new_static("Plain Text"),
            ]
        );

        // Loading an unknown language returns an error.
        assert!(languages.language_for_name("Unknown").await.is_err());
    }

    #[test]
    fn test_deserializing_comments_backwards_compat() {
        // current version of `block_comment` and `documentation_comment` work
        {
            let config: LanguageConfig = ::toml::from_str(
                r#"
                name = "Foo"
                block_comment = { start = "a", end = "b", prefix = "c", tab_size = 1 }
                documentation_comment = { start = "d", end = "e", prefix = "f", tab_size = 2 }
                "#,
            )
            .unwrap();
            assert_matches!(config.block_comment, Some(BlockCommentConfig { .. }));
            assert_matches!(
                config.documentation_comment,
                Some(BlockCommentConfig { .. })
            );

            let block_config = config.block_comment.unwrap();
            assert_eq!(block_config.start.as_ref(), "a");
            assert_eq!(block_config.end.as_ref(), "b");
            assert_eq!(block_config.prefix.as_ref(), "c");
            assert_eq!(block_config.tab_size, 1);

            let doc_config = config.documentation_comment.unwrap();
            assert_eq!(doc_config.start.as_ref(), "d");
            assert_eq!(doc_config.end.as_ref(), "e");
            assert_eq!(doc_config.prefix.as_ref(), "f");
            assert_eq!(doc_config.tab_size, 2);
        }

        // former `documentation` setting is read into `documentation_comment`
        {
            let config: LanguageConfig = ::toml::from_str(
                r#"
                name = "Foo"
                documentation = { start = "a", end = "b", prefix = "c", tab_size = 1}
                "#,
            )
            .unwrap();
            assert_matches!(
                config.documentation_comment,
                Some(BlockCommentConfig { .. })
            );

            let config = config.documentation_comment.unwrap();
            assert_eq!(config.start.as_ref(), "a");
            assert_eq!(config.end.as_ref(), "b");
            assert_eq!(config.prefix.as_ref(), "c");
            assert_eq!(config.tab_size, 1);
        }

        // old block_comment format is read into BlockCommentConfig
        {
            let config: LanguageConfig = ::toml::from_str(
                r#"
                name = "Foo"
                block_comment = ["a", "b"]
                "#,
            )
            .unwrap();
            assert_matches!(config.block_comment, Some(BlockCommentConfig { .. }));

            let config = config.block_comment.unwrap();
            assert_eq!(config.start.as_ref(), "a");
            assert_eq!(config.end.as_ref(), "b");
            assert_eq!(config.prefix.as_ref(), "");
            assert_eq!(config.tab_size, 0);
        }
    }
}
