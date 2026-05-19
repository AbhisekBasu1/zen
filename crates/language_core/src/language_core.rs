// language_core: tree-sitter grammar infrastructure, language configuration,
// and highlight mapping.

pub mod grammar;
pub mod highlight_map;
pub mod language_config;

pub use grammar::{
    BracketsConfig, BracketsPatternConfig, Grammar, GrammarId, HighlightsConfig, IndentConfig,
    InjectionConfig, InjectionPatternConfig, NEXT_GRAMMAR_ID, OutlineConfig, OverrideConfig,
    OverrideEntry, RedactionConfig, TextObject, TextObjectConfig,
};
pub use highlight_map::{HighlightId, HighlightMap};
pub use language_config::{
    BlockCommentConfig, BracketPair, BracketPairConfig, BracketPairContent, DecreaseIndentConfig,
    LanguageConfig, LanguageConfigOverride, LanguageMatcher, OrderedListConfig, Override, SoftWrap,
    TaskListConfig, WrapCharactersConfig, auto_indent_using_last_non_empty_line_default,
    deserialize_regex, deserialize_regex_vec, regex_json_schema, regex_vec_json_schema,
    serialize_regex,
};

pub mod code_label;
pub mod language_name;
pub mod queries;

pub use code_label::{CodeLabel, CodeLabelBuilder};
pub use language_name::{LanguageId, LanguageName};
pub use queries::{LanguageQueries, QUERY_FILENAME_PREFIXES};
