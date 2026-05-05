use crate::{Project, ProjectEntryId, ProjectItem, ProjectPath};
use anyhow::{Result, anyhow};
use collections::HashSet;
pub use gpui::ImageFormat;
use gpui::{App, AsyncApp, Context, Entity, EventEmitter, Task};
use language::File;
use rpc::{TypedEnvelope, proto};
use std::{num::NonZeroU64, path::PathBuf, sync::Arc};

#[derive(Clone, Copy, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
pub struct ImageId(NonZeroU64);

impl ImageId {
    pub fn to_proto(&self) -> u64 {
        self.0.get()
    }
}

impl std::fmt::Display for ImageId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl From<NonZeroU64> for ImageId {
    fn from(id: NonZeroU64) -> Self {
        Self(id)
    }
}

#[derive(Debug)]
pub enum ImageItemEvent {
    ReloadNeeded,
    Reloaded,
    FileHandleChanged,
    MetadataUpdated,
}

impl EventEmitter<ImageItemEvent> for ImageItem {}

pub enum ImageStoreEvent {
    ImageAdded(Entity<ImageItem>),
}

impl EventEmitter<ImageStoreEvent> for ImageStore {}

#[derive(Debug, Clone, Copy)]
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub file_size: u64,
    pub colors: Option<ImageColorInfo>,
    pub format: ImageFormat,
}

#[derive(Debug, Clone, Copy)]
pub struct ImageColorInfo {
    pub channels: u8,
    pub bits_per_channel: u8,
}

impl ImageColorInfo {
    pub const fn bits_per_pixel(&self) -> u8 {
        self.channels * self.bits_per_channel
    }
}

pub struct ImageItem {
    pub id: ImageId,
    pub file: Arc<worktree::File>,
    pub image: Arc<gpui::Image>,
    pub image_metadata: Option<ImageMetadata>,
}

impl ImageItem {
    pub fn compute_metadata_from_bytes(_: &[u8]) -> Result<ImageMetadata> {
        Err(anyhow!("image viewing is disabled in this build"))
    }

    pub async fn load_image_metadata(
        _: Entity<ImageItem>,
        _: Entity<Project>,
        _: &mut AsyncApp,
    ) -> Result<ImageMetadata> {
        Err(anyhow!("image viewing is disabled in this build"))
    }

    pub fn project_path(&self, cx: &App) -> ProjectPath {
        ProjectPath {
            worktree_id: self.file.worktree_id(cx),
            path: self.file.path().clone(),
        }
    }

    pub fn abs_path(&self, cx: &App) -> Option<PathBuf> {
        Some(self.file.as_local()?.abs_path(cx))
    }
}

pub fn is_image_file(_: &Entity<Project>, _: &ProjectPath, _: &App) -> bool {
    false
}

impl ProjectItem for ImageItem {
    fn try_open(
        _: &Entity<Project>,
        _: &ProjectPath,
        _: &mut App,
    ) -> Option<Task<anyhow::Result<Entity<Self>>>> {
        None
    }

    fn entry_id(&self, _: &App) -> Option<ProjectEntryId> {
        self.file.entry_id
    }

    fn project_path(&self, cx: &App) -> Option<ProjectPath> {
        Some(self.project_path(cx))
    }

    fn is_dirty(&self) -> bool {
        false
    }
}

pub struct ImageStore;

impl ImageStore {
    pub fn local(_: Entity<crate::worktree_store::WorktreeStore>, _: &mut Context<Self>) -> Self {
        Self
    }

    pub fn remote(
        _: Entity<crate::worktree_store::WorktreeStore>,
        _: rpc::AnyProtoClient,
        _: u64,
        _: &mut Context<Self>,
    ) -> Self {
        Self
    }

    pub fn images(&self) -> impl '_ + Iterator<Item = Entity<ImageItem>> {
        std::iter::empty()
    }

    pub fn get(&self, _: ImageId) -> Option<Entity<ImageItem>> {
        None
    }

    pub fn get_by_path(&self, _: &ProjectPath, _: &App) -> Option<Entity<ImageItem>> {
        None
    }

    pub fn open_image(
        &mut self,
        _: ProjectPath,
        _: &mut Context<Self>,
    ) -> Task<Result<Entity<ImageItem>>> {
        Task::ready(Err(anyhow!("image viewing is disabled in this build")))
    }

    pub fn reload_images(
        &self,
        _: HashSet<Entity<ImageItem>>,
        _: &mut Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn handle_create_image_for_peer(
        &mut self,
        _: TypedEnvelope<proto::CreateImageForPeer>,
        _: &mut Context<Self>,
    ) -> Result<()> {
        Ok(())
    }
}
