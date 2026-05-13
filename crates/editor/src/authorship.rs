use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    ops::Range,
    path::{Path, PathBuf},
};

const AUTHORSHIP_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PersistedAuthorship {
    version: u32,
    path: String,
    text: String,
    human_ranges: Vec<PersistedRange>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedRange {
    start: usize,
    end: usize,
}

impl PersistedAuthorship {
    pub(crate) fn previous_text(&self) -> &str {
        &self.text
    }

    pub(crate) fn human_ranges(&self) -> Vec<Range<usize>> {
        self.human_ranges
            .iter()
            .filter_map(|range| (range.start < range.end).then_some(range.start..range.end))
            .collect()
    }
}

pub(crate) fn load(abs_path: &Path) -> Result<Option<PersistedAuthorship>> {
    let path = authorship_path(abs_path);
    if !path.exists() {
        return Ok(None);
    }

    let persisted = serde_json::from_str::<PersistedAuthorship>(
        &fs::read_to_string(&path)
            .with_context(|| format!("reading authorship data from {}", path.display()))?,
    )
    .with_context(|| format!("parsing authorship data from {}", path.display()))?;

    if persisted.version != AUTHORSHIP_VERSION || persisted.path != abs_path.to_string_lossy() {
        return Ok(None);
    }

    Ok(Some(persisted))
}

pub(crate) fn save(abs_path: &Path, text: String, human_ranges: Vec<Range<usize>>) -> Result<()> {
    let path = authorship_path(abs_path);
    let human_ranges = human_ranges
        .into_iter()
        .filter_map(|range| {
            (range.start < range.end).then_some(PersistedRange {
                start: range.start,
                end: range.end,
            })
        })
        .collect::<Vec<_>>();

    if human_ranges.is_empty() {
        delete(abs_path)?;
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating authorship directory {}", parent.display()))?;
    }

    let persisted = PersistedAuthorship {
        version: AUTHORSHIP_VERSION,
        path: abs_path.to_string_lossy().into_owned(),
        text,
        human_ranges,
    };

    fs::write(&path, serde_json::to_string(&persisted)?)
        .with_context(|| format!("writing authorship data to {}", path.display()))?;
    Ok(())
}

pub(crate) fn delete(abs_path: &Path) -> Result<()> {
    let path = authorship_path(abs_path);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("deleting authorship data at {}", path.display()))
        }
    }
}

fn authorship_path(abs_path: &Path) -> PathBuf {
    paths::data_dir().join("authorship").join(format!(
        "{:016x}.json",
        stable_hash(abs_path.to_string_lossy().as_bytes())
    ))
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
