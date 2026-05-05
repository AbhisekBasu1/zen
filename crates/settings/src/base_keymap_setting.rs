use std::fmt::{Display, Formatter};

use crate::{self as settings, settings_content::BaseKeymapContent};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::{RegisterSetting, Settings};

/// Base key bindings scheme. Base keymaps can be overridden with user keymaps.
///
/// Default: VSCode
#[derive(
    Copy, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default, RegisterSetting,
)]
pub enum BaseKeymap {
    #[default]
    VSCode,
    JetBrains,
    SublimeText,
    Atom,
    TextMate,
    Emacs,
    Cursor,
    None,
}

impl From<BaseKeymapContent> for BaseKeymap {
    fn from(value: BaseKeymapContent) -> Self {
        match value {
            BaseKeymapContent::VSCode => Self::VSCode,
            BaseKeymapContent::JetBrains => Self::JetBrains,
            BaseKeymapContent::SublimeText => Self::SublimeText,
            BaseKeymapContent::Atom => Self::Atom,
            BaseKeymapContent::TextMate => Self::TextMate,
            BaseKeymapContent::Emacs => Self::Emacs,
            BaseKeymapContent::Cursor => Self::Cursor,
            BaseKeymapContent::None => Self::None,
        }
    }
}
impl Into<BaseKeymapContent> for BaseKeymap {
    fn into(self) -> BaseKeymapContent {
        match self {
            BaseKeymap::VSCode => BaseKeymapContent::VSCode,
            BaseKeymap::JetBrains => BaseKeymapContent::JetBrains,
            BaseKeymap::SublimeText => BaseKeymapContent::SublimeText,
            BaseKeymap::Atom => BaseKeymapContent::Atom,
            BaseKeymap::TextMate => BaseKeymapContent::TextMate,
            BaseKeymap::Emacs => BaseKeymapContent::Emacs,
            BaseKeymap::Cursor => BaseKeymapContent::Cursor,
            BaseKeymap::None => BaseKeymapContent::None,
        }
    }
}

impl Display for BaseKeymap {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BaseKeymap::VSCode => write!(f, "VS Code"),
            BaseKeymap::JetBrains => write!(f, "JetBrains"),
            BaseKeymap::SublimeText => write!(f, "Sublime Text"),
            BaseKeymap::Atom => write!(f, "Atom"),
            BaseKeymap::TextMate => write!(f, "TextMate"),
            BaseKeymap::Emacs => write!(f, "Emacs (beta)"),
            BaseKeymap::Cursor => write!(f, "Cursor (beta)"),
            BaseKeymap::None => write!(f, "None"),
        }
    }
}

impl BaseKeymap {
    #[cfg(target_os = "macos")]
    pub const OPTIONS: [(&'static str, Self); 7] = [
        ("VS Code (Default)", Self::VSCode),
        ("Atom", Self::Atom),
        ("JetBrains", Self::JetBrains),
        ("Sublime Text", Self::SublimeText),
        ("Emacs (beta)", Self::Emacs),
        ("TextMate", Self::TextMate),
        ("Cursor", Self::Cursor),
    ];

    #[cfg(not(target_os = "macos"))]
    pub const OPTIONS: [(&'static str, Self); 6] = [
        ("VS Code (Default)", Self::VSCode),
        ("Atom", Self::Atom),
        ("JetBrains", Self::JetBrains),
        ("Sublime Text", Self::SublimeText),
        ("Emacs (beta)", Self::Emacs),
        ("Cursor", Self::Cursor),
    ];

    pub fn asset_path(&self) -> Option<&'static str> {
        let _ = self;
        None
    }

    pub fn names() -> impl Iterator<Item = &'static str> {
        Self::OPTIONS.iter().map(|(name, _)| *name)
    }

    pub fn from_names(option: &str) -> BaseKeymap {
        Self::OPTIONS
            .iter()
            .copied()
            .find_map(|(name, value)| (name == option).then_some(value))
            .unwrap_or_default()
    }
}

impl Settings for BaseKeymap {
    fn from_settings(s: &crate::settings_content::SettingsContent) -> Self {
        s.base_keymap.unwrap().into()
    }
}
