use gpui::App;
use std::sync::Arc;

pub use language::*;

pub fn init(languages: Arc<LanguageRegistry>, cx: &mut App) {
    #[cfg(feature = "load-grammars")]
    languages.register_native_grammars(grammars::native_grammars());

    for language_name in BUILT_IN_LANGUAGES {
        register_language(&languages, language_name, cx);
    }
}

const BUILT_IN_LANGUAGES: &[&str] = &["json", "jsonc", "markdown", "markdown-inline", "regex"];

fn register_language(languages: &LanguageRegistry, name: &'static str, _cx: &mut App) {
    let config = load_config(name);
    languages.register_language(
        config.name.clone(),
        config.grammar.clone(),
        config.matcher.clone(),
        config.hidden,
        Arc::new(move || {
            Ok(LoadedLanguage {
                config: config.clone(),
                queries: grammars::load_queries(name),
            })
        }),
    );
}

#[cfg(any(test, feature = "test-support"))]
pub fn language(name: &str, grammar: tree_sitter::Language) -> Arc<Language> {
    Arc::new(
        Language::new(grammars::load_config(name), Some(grammar))
            .with_queries(grammars::load_queries(name))
            .unwrap(),
    )
}

fn load_config(name: &str) -> LanguageConfig {
    let grammars_loaded = cfg!(any(feature = "load-grammars", test));
    grammars::load_config_for_feature(name, grammars_loaded)
}
