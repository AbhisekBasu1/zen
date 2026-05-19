use std::sync::{Arc, LazyLock};

use collections::HashMap;
use gpui::SharedString;

use crate::Appearance;

/// A family of icon themes.
pub struct IconThemeFamily {
    /// The unique ID for the icon theme family.
    pub id: String,
    /// The name of the icon theme family.
    pub name: SharedString,
    /// The author of the icon theme family.
    pub author: SharedString,
    /// The list of icon themes in the family.
    pub themes: Vec<IconTheme>,
}

/// An icon theme.
#[derive(Debug, PartialEq)]
pub struct IconTheme {
    /// The unique ID for the icon theme.
    pub id: String,
    /// The name of the icon theme.
    pub name: SharedString,
    /// The appearance of the icon theme (e.g., light or dark).
    pub appearance: Appearance,
    /// The icons used for directories.
    pub directory_icons: DirectoryIcons,
    /// The icons used for named directories.
    pub named_directory_icons: HashMap<String, DirectoryIcons>,
    /// The icons used for chevrons.
    pub chevron_icons: ChevronIcons,
    /// The mapping of file stems to their associated icon keys.
    pub file_stems: HashMap<String, String>,
    /// The mapping of file suffixes to their associated icon keys.
    pub file_suffixes: HashMap<String, String>,
    /// The mapping of icon keys to icon definitions.
    pub file_icons: HashMap<String, IconDefinition>,
}

/// The icons used for directories.
#[derive(Debug, PartialEq, Clone)]
pub struct DirectoryIcons {
    /// The path to the icon to use for a collapsed directory.
    pub collapsed: Option<SharedString>,
    /// The path to the icon to use for an expanded directory.
    pub expanded: Option<SharedString>,
}

/// The icons used for chevrons.
#[derive(Debug, PartialEq)]
pub struct ChevronIcons {
    /// The path to the icon to use for a collapsed chevron.
    pub collapsed: Option<SharedString>,
    /// The path to the icon to use for an expanded chevron.
    pub expanded: Option<SharedString>,
}

/// An icon definition.
#[derive(Debug, PartialEq)]
pub struct IconDefinition {
    /// The path to the icon file.
    pub path: SharedString,
}

const FILE_STEMS_BY_ICON_KEY: &[(&str, &[&str])] =
    &[("vcs", &["COMMIT_EDITMSG", "MERGE_MSG", "TAG_EDITMSG"])];

const FILE_SUFFIXES_BY_ICON_KEY: &[(&str, &[&str])] = &[
    ("json", &["json", "jsonc"]),
    ("markdown", &["markdown", "md"]),
    ("text", &["txt"]),
    ("toml", &["toml"]),
    (
        "vcs",
        &[
            "COMMIT_EDITMSG",
            "EDIT_DESCRIPTION",
            "MERGE_MSG",
            "NOTES_EDITMSG",
            "TAG_EDITMSG",
            "gitattributes",
            "gitignore",
            "gitkeep",
            "gitmodules",
        ],
    ),
];

/// A mapping of a file type identifier to its corresponding icon.
const FILE_ICONS: &[(&str, &str)] = &[
    ("default", "icons/file_icons/file.svg"),
    ("json", "icons/file_icons/code.svg"),
    ("markdown", "icons/file_icons/book.svg"),
    ("text", "icons/file_icons/book.svg"),
    ("toml", "icons/file_icons/toml.svg"),
    ("vcs", "icons/file_icons/git.svg"),
];

/// Returns a mapping of file associations to icon keys.
fn icon_keys_by_association(
    associations_by_icon_key: &[(&str, &[&str])],
) -> HashMap<String, String> {
    let mut icon_keys_by_association = HashMap::default();
    for (icon_key, associations) in associations_by_icon_key {
        for association in *associations {
            icon_keys_by_association.insert(association.to_string(), icon_key.to_string());
        }
    }

    icon_keys_by_association
}

/// The name of the default icon theme.
pub const DEFAULT_ICON_THEME_NAME: &str = "Zen (Default)";

static DEFAULT_ICON_THEME: LazyLock<Arc<IconTheme>> = LazyLock::new(|| {
    Arc::new(IconTheme {
        id: "zen".into(),
        name: DEFAULT_ICON_THEME_NAME.into(),
        appearance: Appearance::Dark,
        directory_icons: DirectoryIcons {
            collapsed: Some("icons/file_icons/folder.svg".into()),
            expanded: Some("icons/file_icons/folder_open.svg".into()),
        },
        named_directory_icons: HashMap::default(),
        chevron_icons: ChevronIcons {
            collapsed: Some("icons/file_icons/chevron_right.svg".into()),
            expanded: Some("icons/file_icons/chevron_down.svg".into()),
        },
        file_stems: icon_keys_by_association(FILE_STEMS_BY_ICON_KEY),
        file_suffixes: icon_keys_by_association(FILE_SUFFIXES_BY_ICON_KEY),
        file_icons: HashMap::from_iter(FILE_ICONS.iter().map(|(ty, path)| {
            (
                ty.to_string(),
                IconDefinition {
                    path: (*path).into(),
                },
            )
        })),
    })
});

/// Returns the default icon theme.
pub fn default_icon_theme() -> Arc<IconTheme> {
    DEFAULT_ICON_THEME.clone()
}
