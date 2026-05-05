use gpui::App;
use std::sync::Arc;

pub use language::*;

/// A shared grammar for plain text, exposed for reuse by downstream crates.
#[cfg(feature = "tree-sitter-gitcommit")]
pub static LANGUAGE_GIT_COMMIT: std::sync::LazyLock<Arc<Language>> =
    std::sync::LazyLock::new(|| {
        Arc::new(Language::new(
            LanguageConfig {
                name: "Git Commit".into(),
                soft_wrap: Some(language::SoftWrap::EditorWidth),
                matcher: LanguageMatcher {
                    path_suffixes: vec!["COMMIT_EDITMSG".to_owned()],
                    first_line_pattern: None,
                    ..LanguageMatcher::default()
                },
                line_comments: vec![Arc::from("#")],
                ..LanguageConfig::default()
            },
            Some(tree_sitter_gitcommit::LANGUAGE.into()),
        ))
    });

pub fn init(languages: Arc<LanguageRegistry>, cx: &mut App) {
    #[cfg(feature = "load-grammars")]
    languages.register_native_grammars(grammars::native_grammars());

    for language_name in BUILT_IN_LANGUAGES {
        register_language(&languages, language_name, cx);
    }
}

const BUILT_IN_LANGUAGES: &[&str] = &[
    "bash",
    "c",
    "cpp",
    "css",
    "diff",
    "go",
    "gomod",
    "gowork",
    "json",
    "jsonc",
    "markdown",
    "markdown-inline",
    "python",
    "rust",
    "tsx",
    "typescript",
    "javascript",
    "jsdoc",
    "regex",
    "yaml",
    "gitcommit",
    "zed-keybind-context",
];

fn register_language(languages: &LanguageRegistry, name: &'static str, _cx: &mut App) {
    let config = load_config(name);
    languages.register_language(
        config.name.clone(),
        config.grammar.clone(),
        config.matcher.clone(),
        config.hidden,
        None,
        Arc::new(move || {
            Ok(LoadedLanguage {
                config: config.clone(),
                queries: grammars::load_queries(name),
                toolchain_provider: None,
                manifest_name: None,
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
