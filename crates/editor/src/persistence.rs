use anyhow::Result;
use fs::MTime;
use gpui::App;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use workspace::{ItemId, WorkspaceId};

#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct SerializedEditor {
    pub(crate) abs_path: Option<PathBuf>,
    pub(crate) contents: Option<String>,
    pub(crate) language: Option<String>,
    pub(crate) mtime: Option<MTime>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EditorDb;

impl EditorDb {
    pub fn global(_: &App) -> Self {
        Self
    }

    pub fn get_serialized_editor(
        &self,
        _: ItemId,
        _: WorkspaceId,
    ) -> Result<Option<SerializedEditor>> {
        Ok(None)
    }

    pub async fn save_serialized_editor(
        &self,
        _: ItemId,
        _: WorkspaceId,
        _: SerializedEditor,
    ) -> Result<()> {
        Ok(())
    }

    pub fn get_scroll_position(
        &self,
        _: ItemId,
        _: WorkspaceId,
    ) -> Result<Option<(u32, f64, f64)>> {
        Ok(None)
    }

    pub async fn save_scroll_position(
        &self,
        _: ItemId,
        _: WorkspaceId,
        _: u32,
        _: f64,
        _: f64,
    ) -> Result<()> {
        Ok(())
    }

    pub fn get_editor_selections(&self, _: ItemId, _: WorkspaceId) -> Result<Vec<(usize, usize)>> {
        Ok(Vec::new())
    }

    pub fn get_editor_folds(
        &self,
        _: ItemId,
        _: WorkspaceId,
    ) -> Result<Vec<(usize, usize, Option<String>, Option<String>)>> {
        Ok(Vec::new())
    }

    pub fn get_file_folds(
        &self,
        _: WorkspaceId,
        _: &Path,
    ) -> Result<Vec<(usize, usize, Option<String>, Option<String>)>> {
        Ok(Vec::new())
    }

    pub async fn save_editor_selections(
        &self,
        _: ItemId,
        _: WorkspaceId,
        _: Vec<(usize, usize)>,
    ) -> Result<()> {
        Ok(())
    }

    pub async fn save_file_folds(
        &self,
        _: WorkspaceId,
        _: Arc<Path>,
        _: Vec<(usize, usize, String, String)>,
    ) -> Result<()> {
        Ok(())
    }

    pub async fn delete_file_folds(&self, _: WorkspaceId, _: Arc<Path>) -> Result<()> {
        Ok(())
    }
}
