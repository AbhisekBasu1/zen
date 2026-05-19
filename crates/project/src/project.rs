pub mod buffer_store;
pub mod debounced_delay;
pub mod project_settings;
pub mod trusted_worktrees;
pub mod worktree_store;

use itertools::Itertools;

use crate::worktree_store::WorktreeIdCounter;
pub use worktree_store::WorktreePaths;

use anyhow::{Context as _, Result, anyhow};
use buffer_store::{BufferStore, BufferStoreEvent};
use clock::ReplicaId;

use collections::HashSet;

use futures::{StreamExt, future::try_join_all};

use gpui::{App, AppContext, AsyncApp, Context, Entity, EventEmitter, SharedString, Task};
use language::{Buffer, BufferEvent, Capability, DiskState, Language, LanguageRegistry};
use parking_lot::Mutex;
use project_settings::{SettingsObserver, SettingsObserverEvent};
use settings::{InvalidSettingsError, SettingsLocation};
use std::{
    borrow::Cow,
    future::Future,
    path::{Path, PathBuf},
    str,
    sync::Arc,
};

use text::BufferId;
use util::{
    ResultExt as _,
    path_list::PathList,
    paths::{PathStyle, SanitizedPath, expand_tilde, is_absolute},
    rel_path::RelPath,
};
use worktree::{CreatedEntry, Snapshot, Traversal};
pub use worktree::{
    Entry, EntryKind, FS_WATCH_LATENCY, File, LocalWorktree, PathChange, ProjectEntryId,
    UpdatedEntriesSet, Worktree, WorktreeId, WorktreeSettings,
};
use worktree_store::{WorktreeStore, WorktreeStoreEvent};

pub use buffer_store::ProjectTransaction;
pub use fs::*;
pub use language::Location;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct ProjectId(pub u64);

impl ProjectId {
    pub fn to_proto(self) -> u64 {
        self.0
    }
}
#[derive(Clone, Copy, Debug)]
pub struct LocalProjectFlags {
    pub init_worktree_trust: bool,
    pub watch_global_configs: bool,
}

impl Default for LocalProjectFlags {
    fn default() -> Self {
        Self {
            init_worktree_trust: true,
            watch_global_configs: true,
        }
    }
}

pub trait ProjectItem: 'static {
    fn try_open(
        project: &Entity<Project>,
        path: &ProjectPath,
        cx: &mut App,
    ) -> Option<Task<Result<Entity<Self>>>>
    where
        Self: Sized;
    fn entry_id(&self, cx: &App) -> Option<ProjectEntryId>;
    fn project_path(&self, cx: &App) -> Option<ProjectPath>;
    fn is_dirty(&self) -> bool;
}

#[derive(Clone)]
pub enum OpenedBufferEvent {
    Disconnected,
    Ok(BufferId),
    Err(BufferId, Arc<anyhow::Error>),
}

/// Semantics-aware entity that is relevant to one or more [`Worktree`] with the files.
/// `Project` is responsible for search queries and synchronizing worktree states.
/// Maps [`Worktree`] entries with its own logic using [`ProjectEntryId`] and [`ProjectPath`] structs.
pub struct Project {
    active_entry: Option<ProjectEntryId>,
    languages: Arc<LanguageRegistry>,
    fs: Arc<dyn Fs>,
    worktree_store: Entity<WorktreeStore>,
    buffer_store: Entity<BufferStore>,
    _subscriptions: Vec<gpui::Subscription>,
    _settings_observer: Entity<SettingsObserver>,
    last_worktree_paths: WorktreePaths,
}

/// A link to display in a toast notification, useful to point to documentation.
#[derive(PartialEq, Debug, Clone)]
pub struct ToastLink {
    pub label: &'static str,
    pub url: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Toast {
        notification_id: SharedString,
        message: String,
        /// Optional link to display as a button in the toast.
        link: Option<ToastLink>,
    },
    HideToast {
        notification_id: SharedString,
    },
    ActiveEntryChanged(Option<ProjectEntryId>),
    ActivateProjectPanel,
    WorktreeAdded(WorktreeId),
    WorktreeOrderChanged,
    WorktreeRemoved(WorktreeId),
    WorktreeUpdatedEntries(WorktreeId, UpdatedEntriesSet),
    WorktreePathsChanged {
        old_worktree_paths: WorktreePaths,
    },
    Closed,
    DeletedEntry(WorktreeId, ProjectEntryId),
    RevealInProjectPanel(ProjectEntryId),
    ExpandedAllForEntry(WorktreeId, ProjectEntryId),
    EntryRenamed(ProjectTransaction, ProjectPath, PathBuf),
    BufferEdited,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct ProjectPath {
    pub worktree_id: WorktreeId,
    pub path: Arc<RelPath>,
}

impl ProjectPath {
    pub fn from_file(value: &dyn language::File, cx: &App) -> Self {
        ProjectPath {
            worktree_id: value.worktree_id(cx),
            path: value.path().clone(),
        }
    }

    pub fn root_path(worktree_id: WorktreeId) -> Self {
        Self {
            worktree_id,
            path: RelPath::empty().into(),
        }
    }

    pub fn starts_with(&self, other: &ProjectPath) -> bool {
        self.worktree_id == other.worktree_id && self.path.starts_with(&other.path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InlayId {
    EditPrediction(usize),
    Hint(usize),
}

impl InlayId {
    pub fn id(&self) -> usize {
        match self {
            Self::EditPrediction(id) => *id,
            Self::Hint(id) => *id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DirectoryItem {
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Clone)]
pub enum DirectoryLister {
    Project(Entity<Project>),
    Local(Entity<Project>, Arc<dyn Fs>),
}

impl std::fmt::Debug for DirectoryLister {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DirectoryLister::Project(project) => {
                write!(f, "DirectoryLister::Project({project:?})")
            }
            DirectoryLister::Local(project, _) => {
                write!(f, "DirectoryLister::Local({project:?})")
            }
        }
    }
}

impl DirectoryLister {
    pub fn resolve_tilde<'a>(&self, path: &'a String, _cx: &App) -> Cow<'a, str> {
        expand_tilde(path)
    }

    pub fn default_query(&self, cx: &mut App) -> String {
        let project = match self {
            DirectoryLister::Project(project) => project,
            DirectoryLister::Local(project, _) => project,
        }
        .read(cx);
        let path_style = project.path_style(cx);
        project
            .visible_worktrees(cx)
            .next()
            .map(|worktree| worktree.read(cx).abs_path().to_string_lossy().into_owned())
            .or_else(|| std::env::home_dir().map(|dir| dir.to_string_lossy().into_owned()))
            .map(|mut s| {
                s.push_str(path_style.primary_separator());
                s
            })
            .unwrap_or_else(|| {
                if path_style.is_windows() {
                    "C:\\"
                } else {
                    "~/"
                }
                .to_string()
            })
    }

    pub fn list_directory(&self, path: String, cx: &mut App) -> Task<Result<Vec<DirectoryItem>>> {
        match self {
            DirectoryLister::Project(project) => {
                project.update(cx, |project, cx| project.list_directory(path, cx))
            }
            DirectoryLister::Local(_, fs) => {
                let fs = fs.clone();
                cx.background_spawn(async move {
                    let mut results = vec![];
                    let expanded = expand_tilde(&path);
                    let query = Path::new(expanded.as_ref());
                    let mut response = fs.read_dir(query).await?;
                    while let Some(path) = response.next().await {
                        let path = path?;
                        if let Some(file_name) = path.file_name() {
                            results.push(DirectoryItem {
                                path: PathBuf::from(file_name.to_os_string()),
                                is_dir: fs.is_dir(&path).await,
                            });
                        }
                    }
                    Ok(results)
                })
            }
        }
    }

    pub fn path_style(&self, cx: &App) -> PathStyle {
        match self {
            Self::Local(project, ..) | Self::Project(project, ..) => {
                project.read(cx).path_style(cx)
            }
        }
    }
}

pub const CURRENT_PROJECT_FEATURES: &[&str] = &["new-style-anchors"];

impl Project {
    pub fn init(_: &mut App) {}

    pub fn local(
        languages: Arc<LanguageRegistry>,
        fs: Arc<dyn Fs>,
        flags: LocalProjectFlags,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let worktree_store =
                cx.new(|cx| WorktreeStore::local(false, fs.clone(), WorktreeIdCounter::get(cx)));
            if flags.init_worktree_trust {
                trusted_worktrees::track_worktree_trust(worktree_store.clone(), cx);
            }
            cx.subscribe(&worktree_store, Self::on_worktree_store_event)
                .detach();

            let buffer_store = cx.new(|cx| BufferStore::local(worktree_store.clone(), cx));
            cx.subscribe(&buffer_store, Self::on_buffer_store_event)
                .detach();

            let settings_observer = cx.new(|cx| {
                SettingsObserver::new_local(
                    fs.clone(),
                    worktree_store.clone(),
                    flags.watch_global_configs,
                    cx,
                )
            });
            cx.subscribe(&settings_observer, Self::on_settings_observer_event)
                .detach();

            Self {
                worktree_store,
                buffer_store,
                _subscriptions: vec![cx.on_release(Self::release)],
                active_entry: None,
                languages,
                _settings_observer: settings_observer,
                fs,

                last_worktree_paths: WorktreePaths::default(),
            }
        })
    }

    fn release(&mut self, _: &mut App) {}

    #[cfg(feature = "test-support")]
    pub async fn example(
        root_paths: impl IntoIterator<Item = &Path>,
        cx: &mut AsyncApp,
    ) -> Entity<Project> {
        let fs = Arc::new(RealFs::new(cx.background_executor().clone()));
        let languages = LanguageRegistry::test(cx.background_executor().clone());
        let project = cx.update(|cx| {
            Project::local(
                Arc::new(languages),
                fs,
                LocalProjectFlags {
                    init_worktree_trust: false,
                    ..Default::default()
                },
                cx,
            )
        });
        for path in root_paths {
            let (tree, _): (Entity<Worktree>, _) = project
                .update(cx, |project, cx| {
                    project.find_or_create_worktree(path, true, cx)
                })
                .await
                .unwrap();
            tree.read_with(cx, |tree, _| tree.as_local().unwrap().scan_complete())
                .await;
        }
        project
    }

    #[cfg(feature = "test-support")]
    pub async fn test(
        fs: Arc<dyn Fs>,
        root_paths: impl IntoIterator<Item = &Path>,
        cx: &mut gpui::TestAppContext,
    ) -> Entity<Project> {
        Self::test_project(fs, root_paths, false, cx).await
    }

    #[cfg(feature = "test-support")]
    pub async fn test_with_worktree_trust(
        fs: Arc<dyn Fs>,
        root_paths: impl IntoIterator<Item = &Path>,
        cx: &mut gpui::TestAppContext,
    ) -> Entity<Project> {
        Self::test_project(fs, root_paths, true, cx).await
    }

    #[cfg(feature = "test-support")]
    async fn test_project(
        fs: Arc<dyn Fs>,
        root_paths: impl IntoIterator<Item = &Path>,
        init_worktree_trust: bool,
        cx: &mut gpui::TestAppContext,
    ) -> Entity<Project> {
        let languages = LanguageRegistry::test(cx.executor());
        let project = cx.update(|cx| {
            Project::local(
                Arc::new(languages),
                fs,
                LocalProjectFlags {
                    init_worktree_trust,
                    ..Default::default()
                },
                cx,
            )
        });
        for path in root_paths {
            let (tree, _) = project
                .update(cx, |project, cx| {
                    project.find_or_create_worktree(path, true, cx)
                })
                .await
                .unwrap();

            tree.read_with(cx, |tree, _| tree.as_local().unwrap().scan_complete())
                .await;
        }
        project
    }

    #[inline]
    pub fn worktree_store(&self) -> Entity<WorktreeStore> {
        self.worktree_store.clone()
    }

    /// Returns a future that resolves when all visible worktrees have completed
    /// their initial scan.
    pub fn wait_for_initial_scan(&self, cx: &App) -> impl Future<Output = ()> + use<> {
        self.worktree_store.read(cx).wait_for_initial_scan()
    }

    #[inline]
    pub fn buffer_for_id(&self, remote_id: BufferId, cx: &App) -> Option<Entity<Buffer>> {
        self.buffer_store.read(cx).get(remote_id)
    }

    #[inline]
    pub fn languages(&self) -> &Arc<LanguageRegistry> {
        &self.languages
    }

    #[inline]
    pub fn opened_buffers(&self, cx: &App) -> Vec<Entity<Buffer>> {
        self.buffer_store.read(cx).buffers().collect()
    }

    #[cfg(feature = "test-support")]
    #[inline]
    pub fn has_open_buffer(&self, path: impl Into<ProjectPath>, cx: &App) -> bool {
        self.buffer_store
            .read(cx)
            .get_by_path(&path.into())
            .is_some()
    }

    #[inline]
    pub fn fs(&self) -> &Arc<dyn Fs> {
        &self.fs
    }

    #[inline]
    pub fn remote_id(&self) -> Option<u64> {
        None
    }

    pub fn reveal_path(&self, path: &Path, cx: &mut Context<Self>) {
        cx.reveal_path(path);
    }

    #[inline]
    pub fn replica_id(&self) -> ReplicaId {
        ReplicaId::LOCAL
    }

    /// Collect all worktrees, including ones that don't appear in the project panel
    #[inline]
    pub fn worktrees<'a>(
        &self,
        cx: &'a App,
    ) -> impl 'a + DoubleEndedIterator<Item = Entity<Worktree>> {
        self.worktree_store.read(cx).worktrees()
    }

    /// Collect all user-visible worktrees, the ones that appear in the project panel.
    #[inline]
    pub fn visible_worktrees<'a>(
        &'a self,
        cx: &'a App,
    ) -> impl 'a + DoubleEndedIterator<Item = Entity<Worktree>> {
        self.worktree_store.read(cx).visible_worktrees(cx)
    }

    pub(crate) fn default_visible_worktree_paths(
        worktree_store: &WorktreeStore,
        cx: &App,
    ) -> Vec<PathBuf> {
        worktree_store
            .visible_worktrees(cx)
            .sorted_by(|left, right| {
                left.read(cx)
                    .is_single_file()
                    .cmp(&right.read(cx).is_single_file())
            })
            .filter_map(|worktree| {
                let worktree = worktree.read(cx);
                let path = worktree.abs_path();
                if worktree.is_single_file() {
                    Some(path.parent()?.to_path_buf())
                } else {
                    Some(path.to_path_buf())
                }
            })
            .collect()
    }

    pub fn default_path_list(&self, cx: &App) -> PathList {
        let worktree_roots =
            Self::default_visible_worktree_paths(&self.worktree_store.read(cx), cx);

        if worktree_roots.is_empty() {
            PathList::new(&[paths::home_dir().as_path()])
        } else {
            PathList::new(&worktree_roots)
        }
    }

    #[inline]
    pub fn worktree_for_root_name(&self, root_name: &str, cx: &App) -> Option<Entity<Worktree>> {
        self.visible_worktrees(cx)
            .find(|tree| tree.read(cx).root_name() == root_name)
    }

    fn emit_group_key_changed_if_needed(&mut self, cx: &mut Context<Self>) {
        let new_worktree_paths = self.worktree_paths(cx);
        if new_worktree_paths != self.last_worktree_paths {
            let old_worktree_paths =
                std::mem::replace(&mut self.last_worktree_paths, new_worktree_paths);
            cx.emit(Event::WorktreePathsChanged { old_worktree_paths });
        }
    }

    #[inline]
    pub fn worktree_root_names<'a>(&'a self, cx: &'a App) -> impl Iterator<Item = &'a str> {
        self.visible_worktrees(cx)
            .map(|tree| tree.read(cx).root_name().as_unix_str())
    }

    #[inline]
    pub fn worktree_for_id(&self, id: WorktreeId, cx: &App) -> Option<Entity<Worktree>> {
        self.worktree_store.read(cx).worktree_for_id(id, cx)
    }

    pub fn worktree_for_entry(
        &self,
        entry_id: ProjectEntryId,
        cx: &App,
    ) -> Option<Entity<Worktree>> {
        self.worktree_store
            .read(cx)
            .worktree_for_entry(entry_id, cx)
    }

    #[inline]
    pub fn worktree_id_for_entry(&self, entry_id: ProjectEntryId, cx: &App) -> Option<WorktreeId> {
        self.worktree_for_entry(entry_id, cx)
            .map(|worktree| worktree.read(cx).id())
    }

    /// Checks if the entry is the root of a worktree.
    #[inline]
    pub fn entry_is_worktree_root(&self, entry_id: ProjectEntryId, cx: &App) -> bool {
        self.worktree_for_entry(entry_id, cx)
            .map(|worktree| {
                worktree
                    .read(cx)
                    .root_entry()
                    .is_some_and(|e| e.id == entry_id)
            })
            .unwrap_or(false)
    }

    #[inline]
    pub fn visibility_for_paths(
        &self,
        paths: &[PathBuf],
        exclude_sub_dirs: bool,
        cx: &App,
    ) -> Option<bool> {
        paths
            .iter()
            .map(|path| self.visibility_for_path(path, exclude_sub_dirs, cx))
            .max()
            .flatten()
    }

    pub fn visibility_for_path(
        &self,
        path: &Path,
        exclude_sub_dirs: bool,
        cx: &App,
    ) -> Option<bool> {
        let path = SanitizedPath::new(path).as_path();
        let path_style = self.path_style(cx);
        self.worktrees(cx)
            .filter_map(|worktree| {
                let worktree = worktree.read(cx);
                let abs_path = worktree.abs_path();
                let relative_path = path_style.strip_prefix(path, abs_path.as_ref());
                let is_dir = relative_path
                    .as_ref()
                    .and_then(|p| worktree.entry_for_path(p))
                    .is_some_and(|e| e.is_dir());
                // Don't exclude the worktree root itself, only actual subdirectories
                let is_subdir = relative_path
                    .as_ref()
                    .is_some_and(|p| !p.as_ref().as_unix_str().is_empty());
                let contains =
                    relative_path.is_some() && (!exclude_sub_dirs || !is_dir || !is_subdir);
                contains.then(|| worktree.is_visible())
            })
            .max()
    }

    pub fn create_entry(
        &mut self,
        project_path: impl Into<ProjectPath>,
        is_directory: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<CreatedEntry>> {
        let project_path = project_path.into();
        let Some(worktree) = self.worktree_for_id(project_path.worktree_id, cx) else {
            return Task::ready(Err(anyhow!(format!(
                "No worktree for path {project_path:?}"
            ))));
        };
        worktree.update(cx, |worktree, cx| {
            worktree.create_entry(project_path.path, is_directory, None, cx)
        })
    }

    #[inline]
    pub fn copy_entry(
        &mut self,
        entry_id: ProjectEntryId,
        new_project_path: ProjectPath,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<Entry>>> {
        self.worktree_store.update(cx, |worktree_store, cx| {
            worktree_store.copy_entry(entry_id, new_project_path, cx)
        })
    }

    /// Renames the project entry with given `entry_id`.
    ///
    /// `new_path` is a relative path to worktree root.
    /// If root entry is renamed then its new root name is used instead.
    pub fn rename_entry(
        &mut self,
        entry_id: ProjectEntryId,
        new_path: ProjectPath,
        cx: &mut Context<Self>,
    ) -> Task<Result<CreatedEntry>> {
        let worktree_store = self.worktree_store.clone();
        let Some(worktree) = worktree_store
            .read(cx)
            .worktree_and_entry_for_id(entry_id, cx)
            .map(|(worktree, _)| worktree)
        else {
            return Task::ready(Err(anyhow!(format!("No worktree for entry {entry_id:?}"))));
        };

        let is_root_entry = self.entry_is_worktree_root(entry_id, cx);

        cx.spawn(async move |project, cx| {
            let new_abs_path = {
                let root_path = worktree.read_with(cx, |this, _| this.abs_path());
                if is_root_entry {
                    root_path
                        .parent()
                        .unwrap()
                        .join(new_path.path.as_std_path())
                } else {
                    root_path.join(&new_path.path.as_std_path())
                }
            };
            let transaction = ProjectTransaction::default();

            let entry = worktree_store
                .update(cx, |worktree_store, cx| {
                    worktree_store.rename_entry(entry_id, new_path.clone(), cx)
                })
                .await?;

            project
                .update(cx, |_, cx| {
                    cx.emit(Event::EntryRenamed(
                        transaction,
                        new_path.clone(),
                        new_abs_path.clone(),
                    ));
                })
                .ok();

            Ok(entry)
        })
    }

    #[inline]
    pub fn delete_file(
        &mut self,
        path: ProjectPath,
        trash: bool,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<Option<TrashedEntry>>>> {
        let entry = self.entry_for_path(&path, cx)?;
        self.delete_entry(entry.id, trash, cx)
    }

    #[inline]
    pub fn delete_entry(
        &mut self,
        entry_id: ProjectEntryId,
        trash: bool,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<Option<TrashedEntry>>>> {
        let worktree = self.worktree_for_entry(entry_id, cx)?;
        cx.emit(Event::DeletedEntry(worktree.read(cx).id(), entry_id));
        worktree.update(cx, |worktree, cx| {
            worktree.delete_entry(entry_id, trash, cx)
        })
    }

    #[inline]
    pub fn restore_entry(
        &self,
        worktree_id: WorktreeId,
        trash_entry: TrashedEntry,
        cx: &mut Context<'_, Self>,
    ) -> Task<Result<ProjectPath>> {
        let Some(worktree) = self.worktree_for_id(worktree_id, cx) else {
            return Task::ready(Err(anyhow!("No worktree for id {worktree_id:?}")));
        };

        cx.spawn(async move |_, cx| {
            Worktree::restore_entry(trash_entry, worktree, cx)
                .await
                .map(|rel_path_buf| ProjectPath {
                    worktree_id: worktree_id,
                    path: Arc::from(rel_path_buf.as_rel_path()),
                })
        })
    }

    #[inline]
    pub fn expand_entry(
        &mut self,
        worktree_id: WorktreeId,
        entry_id: ProjectEntryId,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<()>>> {
        let worktree = self.worktree_for_id(worktree_id, cx)?;
        worktree.update(cx, |worktree, cx| worktree.expand_entry(entry_id, cx))
    }

    pub fn expand_all_for_entry(
        &mut self,
        worktree_id: WorktreeId,
        entry_id: ProjectEntryId,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<()>>> {
        let worktree = self.worktree_for_id(worktree_id, cx)?;
        let task = worktree.update(cx, |worktree, cx| {
            worktree.expand_all_for_entry(entry_id, cx)
        });
        Some(cx.spawn(async move |this, cx| {
            task.context("no task")?.await?;
            this.update(cx, |_, cx| {
                cx.emit(Event::ExpandedAllForEntry(worktree_id, entry_id));
            })?;
            Ok(())
        }))
    }

    #[inline]
    pub fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(Event::Closed);
    }

    #[inline]
    pub fn capability(&self) -> Capability {
        Capability::ReadWrite
    }

    #[inline]
    pub fn is_read_only(&self, _: &App) -> bool {
        !self.capability().editable()
    }

    pub fn disable_worktree_scanner(&mut self, cx: &mut Context<Self>) {
        self.worktree_store.update(cx, |worktree_store, _cx| {
            worktree_store.disable_scanner();
        });
    }

    #[inline]
    pub fn create_buffer(
        &mut self,
        language: Option<Arc<Language>>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Buffer>>> {
        self.buffer_store.update(cx, |buffer_store, cx| {
            buffer_store.create_buffer(language, cx)
        })
    }

    #[inline]
    pub fn create_local_buffer(
        &mut self,
        text: &str,
        language: Option<Arc<Language>>,
        cx: &mut Context<Self>,
    ) -> Entity<Buffer> {
        self.buffer_store.update(cx, |buffer_store, cx| {
            buffer_store.create_local_buffer(text, language, cx)
        })
    }

    pub fn open_path(
        &mut self,
        path: ProjectPath,
        cx: &mut Context<Self>,
    ) -> Task<Result<(Option<ProjectEntryId>, Entity<Buffer>)>> {
        let task = self.open_buffer(path, cx);
        cx.spawn(async move |_project, cx| {
            let buffer = task.await?;
            let project_entry_id = buffer.read_with(cx, |buffer, _cx| {
                File::from_dyn(buffer.file()).and_then(|file| file.project_entry_id())
            });

            Ok((project_entry_id, buffer))
        })
    }

    pub fn open_local_buffer(
        &mut self,
        abs_path: impl AsRef<Path>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Buffer>>> {
        let worktree_task = self.find_or_create_worktree(abs_path.as_ref(), false, cx);
        cx.spawn(async move |this, cx| {
            let (worktree, relative_path) = worktree_task.await?;
            this.update(cx, |this, cx| {
                this.open_buffer((worktree.read(cx).id(), relative_path), cx)
            })?
            .await
        })
    }

    #[ztracing::instrument(skip_all)]
    pub fn open_buffer(
        &mut self,
        path: impl Into<ProjectPath>,
        cx: &mut App,
    ) -> Task<Result<Entity<Buffer>>> {
        self.buffer_store.update(cx, |buffer_store, cx| {
            buffer_store.open_buffer(path.into(), cx)
        })
    }

    pub fn open_buffer_by_id(
        &mut self,
        id: BufferId,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Buffer>>> {
        if let Some(buffer) = self.buffer_for_id(id, cx) {
            Task::ready(Ok(buffer))
        } else {
            Task::ready(Err(anyhow!("buffer {id} does not exist")))
        }
    }

    pub fn save_buffers(
        &self,
        buffers: HashSet<Entity<Buffer>>,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        cx.spawn(async move |this, cx| {
            let save_tasks = buffers.into_iter().filter_map(|buffer| {
                this.update(cx, |this, cx| this.save_buffer(buffer, cx))
                    .ok()
            });
            try_join_all(save_tasks).await?;
            Ok(())
        })
    }

    pub fn save_buffer(&self, buffer: Entity<Buffer>, cx: &mut Context<Self>) -> Task<Result<()>> {
        self.buffer_store
            .update(cx, |buffer_store, cx| buffer_store.save_buffer(buffer, cx))
    }

    pub fn save_buffer_as(
        &mut self,
        buffer: Entity<Buffer>,
        path: ProjectPath,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        self.buffer_store.update(cx, |buffer_store, cx| {
            buffer_store.save_buffer_as(buffer.clone(), path, cx)
        })
    }

    pub fn get_open_buffer(&self, path: &ProjectPath, cx: &App) -> Option<Entity<Buffer>> {
        self.buffer_store.read(cx).get_by_path(path)
    }

    fn register_buffer(&mut self, buffer: &Entity<Buffer>, cx: &mut Context<Self>) -> Result<()> {
        cx.subscribe(buffer, |this, buffer, event, cx| {
            this.on_buffer_event(buffer, event, cx);
        })
        .detach();

        Ok(())
    }

    fn on_buffer_store_event(
        &mut self,
        _: Entity<BufferStore>,
        event: &BufferStoreEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            BufferStoreEvent::BufferAdded(buffer) => {
                self.register_buffer(buffer, cx).log_err();
            }
            BufferStoreEvent::BufferDropped(_) => {}
            _ => {}
        }
    }

    fn on_settings_observer_event(
        &mut self,
        _: Entity<SettingsObserver>,
        event: &SettingsObserverEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            SettingsObserverEvent::LocalSettingsUpdated(result) => match result {
                Err(InvalidSettingsError::LocalSettings { message, path }) => {
                    let message = format!("Failed to set local settings in {path:?}:\n{message}");
                    cx.emit(Event::Toast {
                        notification_id: format!("local-settings-{path:?}").into(),
                        link: None,
                        message,
                    });
                }
                Ok(path) => cx.emit(Event::HideToast {
                    notification_id: format!("local-settings-{path:?}").into(),
                }),
                Err(_) => {}
            },
        }
    }

    fn on_worktree_store_event(
        &mut self,
        _: Entity<WorktreeStore>,
        event: &WorktreeStoreEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            WorktreeStoreEvent::WorktreeAdded(worktree) => {
                self.on_worktree_added(worktree, cx);
                cx.emit(Event::WorktreeAdded(worktree.read(cx).id()));
                self.emit_group_key_changed_if_needed(cx);
            }
            WorktreeStoreEvent::WorktreeRemoved(_, id) => {
                cx.emit(Event::WorktreeRemoved(*id));
                self.emit_group_key_changed_if_needed(cx);
            }
            WorktreeStoreEvent::WorktreeReleased(_, id) => {
                self.on_worktree_released(*id, cx);
            }
            WorktreeStoreEvent::WorktreeOrderChanged => cx.emit(Event::WorktreeOrderChanged),
            WorktreeStoreEvent::WorktreeUpdatedEntries(worktree_id, changes) => {
                cx.emit(Event::WorktreeUpdatedEntries(*worktree_id, changes.clone()))
            }
            WorktreeStoreEvent::WorktreeDeletedEntry(worktree_id, id) => {
                cx.emit(Event::DeletedEntry(*worktree_id, *id))
            }
        }
    }

    fn on_worktree_added(&mut self, _: &Entity<Worktree>, _: &mut Context<Self>) {}

    fn on_worktree_released(&mut self, _: WorktreeId, _: &mut Context<Self>) {}

    fn on_buffer_event(
        &mut self,
        buffer: Entity<Buffer>,
        event: &BufferEvent,
        cx: &mut Context<Self>,
    ) -> Option<()> {
        if matches!(event, BufferEvent::Edited { .. }) {
            cx.emit(Event::BufferEdited);
        }

        match event {
            BufferEvent::ReloadNeeded => {
                self.reload_buffers([buffer.clone()].into_iter().collect(), true, cx)
                    .detach_and_log_err(cx);
            }
            BufferEvent::Operation { is_local: true, .. } => {}

            _ => {}
        }

        None
    }

    pub fn reload_buffers(
        &self,
        buffers: HashSet<Entity<Buffer>>,
        push_to_history: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<ProjectTransaction>> {
        self.buffer_store.update(cx, |buffer_store, cx| {
            buffer_store.reload_buffers(buffers, push_to_history, cx)
        })
    }

    /// Move a worktree to a new position in the worktree order.
    ///
    /// The worktree will moved to the opposite side of the destination worktree.
    ///
    /// # Example
    ///
    /// Given the worktree order `[11, 22, 33]` and a call to move worktree `22` to `33`,
    /// worktree_order will be updated to produce the indexes `[11, 33, 22]`.
    ///
    /// Given the worktree order `[11, 22, 33]` and a call to move worktree `22` to `11`,
    /// worktree_order will be updated to produce the indexes `[22, 11, 33]`.
    ///
    /// # Errors
    ///
    /// An error will be returned if the worktree or destination worktree are not found.
    pub fn move_worktree(
        &mut self,
        source: WorktreeId,
        destination: WorktreeId,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.worktree_store.update(cx, |worktree_store, cx| {
            worktree_store.move_worktree(source, destination, cx)
        })
    }

    pub fn try_windows_path_to_wsl(
        &self,
        abs_path: &Path,
        _: &App,
    ) -> impl Future<Output = Result<PathBuf>> + use<> {
        let path = abs_path.to_owned();
        async move { Ok(path) }
    }

    pub fn find_or_create_worktree(
        &mut self,
        abs_path: impl AsRef<Path>,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<(Entity<Worktree>, Arc<RelPath>)>> {
        self.worktree_store.update(cx, |worktree_store, cx| {
            worktree_store.find_or_create_worktree(abs_path, visible, cx)
        })
    }

    pub fn find_worktree(
        &self,
        abs_path: &Path,
        cx: &App,
    ) -> Option<(Entity<Worktree>, Arc<RelPath>)> {
        self.worktree_store.read(cx).find_worktree(abs_path, cx)
    }

    /// Returns the resolved version of `path`, that was found in `buffer`, if it exists.
    pub fn resolve_path_in_buffer(
        &self,
        path: &str,
        buffer: &Entity<Buffer>,
        cx: &mut Context<Self>,
    ) -> Task<Option<ResolvedPath>> {
        if util::paths::is_absolute(path, self.path_style(cx)) || path.starts_with("~") {
            self.resolve_abs_path(path, cx)
        } else {
            self.resolve_path_in_worktrees(path, buffer, cx)
        }
    }

    pub fn resolve_abs_file_path(
        &self,
        path: &str,
        cx: &mut Context<Self>,
    ) -> Task<Option<ResolvedPath>> {
        let resolve_task = self.resolve_abs_path(path, cx);
        cx.background_spawn(async move {
            let resolved_path = resolve_task.await;
            resolved_path.filter(|path| path.is_file())
        })
    }

    pub fn resolve_abs_path(&self, path: &str, cx: &App) -> Task<Option<ResolvedPath>> {
        let expanded = PathBuf::from(expand_tilde(path).into_owned());
        let fs = self.fs.clone();
        cx.background_spawn(async move {
            let metadata = fs.metadata(&expanded).await.ok().flatten();

            metadata.map(|metadata| ResolvedPath::AbsPath {
                path: expanded.to_string_lossy().into_owned(),
                is_dir: metadata.is_dir,
            })
        })
    }

    fn resolve_path_in_worktrees(
        &self,
        path: &str,
        buffer: &Entity<Buffer>,
        cx: &mut Context<Self>,
    ) -> Task<Option<ResolvedPath>> {
        let mut candidates = vec![];
        let path_style = self.path_style(cx);
        if let Ok(path) = RelPath::new(path.as_ref(), path_style) {
            candidates.push(path.into_arc());
        }

        if let Some(file) = buffer.read(cx).file()
            && let Some(dir) = file.path().parent()
        {
            if let Some(joined) = path_style.join(&*dir.display(path_style), path)
                && let Some(joined) = RelPath::new(joined.as_ref(), path_style).ok()
            {
                candidates.push(joined.into_arc());
            }
        }

        let buffer_worktree_id = buffer.read(cx).file().map(|file| file.worktree_id(cx));
        let worktrees_with_ids: Vec<_> = self
            .worktrees(cx)
            .map(|worktree| {
                let id = worktree.read(cx).id();
                (worktree, id)
            })
            .collect();

        cx.spawn(async move |_, cx| {
            if let Some(buffer_worktree_id) = buffer_worktree_id
                && let Some((worktree, _)) = worktrees_with_ids
                    .iter()
                    .find(|(_, id)| *id == buffer_worktree_id)
            {
                for candidate in candidates.iter() {
                    if let Some(path) = Self::resolve_path_in_worktree(worktree, candidate, cx) {
                        return Some(path);
                    }
                }
            }
            for (worktree, id) in worktrees_with_ids {
                if Some(id) == buffer_worktree_id {
                    continue;
                }
                for candidate in candidates.iter() {
                    if let Some(path) = Self::resolve_path_in_worktree(&worktree, candidate, cx) {
                        return Some(path);
                    }
                }
            }
            None
        })
    }

    fn resolve_path_in_worktree(
        worktree: &Entity<Worktree>,
        path: &RelPath,
        cx: &mut AsyncApp,
    ) -> Option<ResolvedPath> {
        worktree.read_with(cx, |worktree, _| {
            worktree.entry_for_path(path).map(|entry| {
                let project_path = ProjectPath {
                    worktree_id: worktree.id(),
                    path: entry.path.clone(),
                };
                ResolvedPath::ProjectPath {
                    project_path,
                    is_dir: entry.is_dir(),
                }
            })
        })
    }

    pub fn list_directory(
        &self,
        query: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<DirectoryItem>>> {
        DirectoryLister::Local(cx.entity(), self.fs.clone()).list_directory(query, cx)
    }

    pub fn create_worktree(
        &mut self,
        abs_path: impl AsRef<Path>,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Worktree>>> {
        self.worktree_store.update(cx, |worktree_store, cx| {
            worktree_store.create_worktree(abs_path, visible, cx)
        })
    }

    /// Returns a task that resolves when the given worktree's `Entity` is
    /// fully dropped (all strong references released), not merely when
    /// `remove_worktree` is called. `remove_worktree` drops the store's
    /// reference and emits `WorktreeRemoved`, but other code may still
    /// hold a strong handle — the worktree isn't safe to delete from
    /// disk until every handle is gone.
    ///
    /// We use `observe_release` on the specific entity rather than
    /// listening for `WorktreeReleased` events because it's simpler at
    /// the call site (one awaitable task, no subscription / channel /
    /// ID filtering).
    pub fn wait_for_worktree_release(
        &mut self,
        worktree_id: WorktreeId,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let Some(worktree) = self.worktree_for_id(worktree_id, cx) else {
            return Task::ready(Ok(()));
        };

        let (released_tx, released_rx) = futures::channel::oneshot::channel();
        let released_tx = std::sync::Arc::new(Mutex::new(Some(released_tx)));
        let release_subscription =
            cx.observe_release(&worktree, move |_project, _released_worktree, _cx| {
                if let Some(released_tx) = released_tx.lock().take() {
                    let _ = released_tx.send(());
                }
            });

        cx.spawn(async move |_project, _cx| {
            let _release_subscription = release_subscription;
            released_rx
                .await
                .map_err(|_| anyhow!("worktree release observer dropped before release"))?;
            Ok(())
        })
    }

    pub fn remove_worktree(&mut self, id_to_remove: WorktreeId, cx: &mut Context<Self>) {
        self.worktree_store.update(cx, |worktree_store, cx| {
            worktree_store.remove_worktree(id_to_remove, cx);
        });
    }

    pub fn remove_worktree_for_main_worktree_path(
        &mut self,
        path: impl AsRef<Path>,
        cx: &mut Context<Self>,
    ) {
        let path = path.as_ref();
        self.worktree_store.update(cx, |worktree_store, cx| {
            if let Some(worktree) = worktree_store.worktree_for_main_worktree_path(path, cx) {
                worktree_store.remove_worktree(worktree.read(cx).id(), cx);
            }
        });
    }

    pub fn set_active_path(&mut self, entry: Option<ProjectPath>, cx: &mut Context<Self>) {
        let new_active_entry = entry.and_then(|project_path| {
            let worktree = self.worktree_for_id(project_path.worktree_id, cx)?;
            let entry = worktree.read(cx).entry_for_path(&project_path.path)?;
            Some(entry.id)
        });
        if new_active_entry != self.active_entry {
            self.active_entry = new_active_entry;
            cx.emit(Event::ActiveEntryChanged(new_active_entry));
        }
    }

    pub fn active_entry(&self) -> Option<ProjectEntryId> {
        self.active_entry
    }

    pub fn entry_for_path<'a>(&'a self, path: &ProjectPath, cx: &'a App) -> Option<&'a Entry> {
        self.worktree_store.read(cx).entry_for_path(path, cx)
    }

    pub fn path_for_entry(&self, entry_id: ProjectEntryId, cx: &App) -> Option<ProjectPath> {
        let worktree = self.worktree_for_entry(entry_id, cx)?;
        let worktree = worktree.read(cx);
        let worktree_id = worktree.id();
        let path = worktree.entry_for_id(entry_id)?.path.clone();
        Some(ProjectPath { worktree_id, path })
    }

    pub fn absolute_path(&self, project_path: &ProjectPath, cx: &App) -> Option<PathBuf> {
        Some(
            self.worktree_for_id(project_path.worktree_id, cx)?
                .read(cx)
                .absolutize(&project_path.path),
        )
    }

    /// Attempts to find a `ProjectPath` corresponding to the given path. If the path
    /// is a *full path*, meaning it starts with the root name of a worktree, we'll locate
    /// it in that worktree. Otherwise, we'll attempt to find it as a relative path in
    /// the first visible worktree that has an entry for that relative path.
    ///
    /// We use this to resolve edit steps, when there's a chance an LLM may omit the workree
    /// root name from paths.
    ///
    /// # Arguments
    ///
    /// * `path` - An absolute path, or a full path that starts with a worktree root name, or a
    ///   relative path within a visible worktree.
    /// * `cx` - A reference to the `AppContext`.
    ///
    /// # Returns
    ///
    /// Returns `Some(ProjectPath)` if a matching worktree is found, otherwise `None`.
    pub fn find_project_path(&self, path: impl AsRef<Path>, cx: &App) -> Option<ProjectPath> {
        let path_style = self.path_style(cx);
        let path = path.as_ref();
        let worktree_store = self.worktree_store.read(cx);

        if is_absolute(&path.to_string_lossy(), path_style) {
            for worktree in worktree_store.visible_worktrees(cx) {
                let worktree_abs_path = worktree.read(cx).abs_path();

                if let Ok(relative_path) = path.strip_prefix(worktree_abs_path)
                    && let Ok(path) = RelPath::new(relative_path, path_style)
                {
                    return Some(ProjectPath {
                        worktree_id: worktree.read(cx).id(),
                        path: path.into_arc(),
                    });
                }
            }
        } else {
            for worktree in worktree_store.visible_worktrees(cx) {
                let worktree = worktree.read(cx);
                if let Ok(rel_path) = RelPath::new(path, path_style) {
                    if let Some(entry) = worktree.entry_for_path(&rel_path) {
                        return Some(ProjectPath {
                            worktree_id: worktree.id(),
                            path: entry.path.clone(),
                        });
                    }
                }
            }

            for worktree in worktree_store.visible_worktrees(cx) {
                let worktree_root_name = worktree.read(cx).root_name();
                if let Ok(relative_path) = path.strip_prefix(worktree_root_name.as_std_path())
                    && let Ok(path) = RelPath::new(relative_path, path_style)
                {
                    return Some(ProjectPath {
                        worktree_id: worktree.read(cx).id(),
                        path: path.into_arc(),
                    });
                }
            }
        }

        None
    }

    /// If there's only one visible worktree, returns the given worktree-relative path with no prefix.
    ///
    /// Otherwise, returns the full path for the project path (obtained by prefixing the worktree-relative path with the name of the worktree).
    pub fn short_full_path_for_project_path(
        &self,
        project_path: &ProjectPath,
        cx: &App,
    ) -> Option<String> {
        let path_style = self.path_style(cx);
        if self.visible_worktrees(cx).take(2).count() < 2 {
            return Some(project_path.path.display(path_style).to_string());
        }
        self.worktree_for_id(project_path.worktree_id, cx)
            .map(|worktree| {
                let worktree_name = worktree.read(cx).root_name();
                worktree_name
                    .join(&project_path.path)
                    .display(path_style)
                    .to_string()
            })
    }

    pub fn project_path_for_absolute_path(&self, abs_path: &Path, cx: &App) -> Option<ProjectPath> {
        self.worktree_store
            .read(cx)
            .project_path_for_absolute_path(abs_path, cx)
    }

    pub fn get_workspace_root(&self, project_path: &ProjectPath, cx: &App) -> Option<PathBuf> {
        Some(
            self.worktree_for_id(project_path.worktree_id, cx)?
                .read(cx)
                .abs_path()
                .to_path_buf(),
        )
    }

    /// Iterator of all open buffers that have unsaved changes
    pub fn dirty_buffers<'a>(&'a self, cx: &'a App) -> impl Iterator<Item = ProjectPath> + 'a {
        self.buffer_store.read(cx).buffers().filter_map(|buf| {
            let buf = buf.read(cx);
            if buf.is_dirty() {
                buf.project_path(cx)
            } else {
                None
            }
        })
    }

    pub fn buffer_store(&self) -> &Entity<BufferStore> {
        &self.buffer_store
    }

    pub fn path_style(&self, cx: &App) -> PathStyle {
        self.worktree_store.read(cx).path_style()
    }

    pub fn contains_local_settings_file(
        &self,
        worktree_id: WorktreeId,
        rel_path: &RelPath,
        cx: &App,
    ) -> bool {
        self.worktree_for_id(worktree_id, cx)
            .map_or(false, |worktree| {
                worktree.read(cx).entry_for_path(rel_path).is_some()
            })
    }

    pub fn worktree_paths(&self, cx: &App) -> WorktreePaths {
        self.worktree_store.read(cx).paths(cx)
    }
}

pub struct PathMatchCandidateSet {
    pub snapshot: Snapshot,
    pub include_ignored: bool,
    pub include_root_name: bool,
    pub candidates: Candidates,
}

pub enum Candidates {
    /// Only consider directories.
    Directories,
    /// Only consider files.
    Files,
    /// Consider directories and files.
    Entries,
}

impl<'a> fuzzy::PathMatchCandidateSet<'a> for PathMatchCandidateSet {
    type Candidates = PathMatchCandidateSetIter<'a>;

    fn id(&self) -> usize {
        self.snapshot.id().to_usize()
    }

    fn len(&self) -> usize {
        match self.candidates {
            Candidates::Files => {
                if self.include_ignored {
                    self.snapshot.file_count()
                } else {
                    self.snapshot.visible_file_count()
                }
            }

            Candidates::Directories => {
                if self.include_ignored {
                    self.snapshot.dir_count()
                } else {
                    self.snapshot.visible_dir_count()
                }
            }

            Candidates::Entries => {
                if self.include_ignored {
                    self.snapshot.entry_count()
                } else {
                    self.snapshot.visible_entry_count()
                }
            }
        }
    }

    fn prefix(&self) -> Arc<RelPath> {
        if self.snapshot.root_entry().is_some_and(|e| e.is_file()) || self.include_root_name {
            self.snapshot.root_name().into()
        } else {
            RelPath::empty().into()
        }
    }

    fn root_is_file(&self) -> bool {
        self.snapshot.root_entry().is_some_and(|f| f.is_file())
    }

    fn path_style(&self) -> PathStyle {
        self.snapshot.path_style()
    }

    fn candidates(&'a self, start: usize) -> Self::Candidates {
        PathMatchCandidateSetIter {
            traversal: match self.candidates {
                Candidates::Directories => self.snapshot.directories(self.include_ignored, start),
                Candidates::Files => self.snapshot.files(self.include_ignored, start),
                Candidates::Entries => self.snapshot.entries(self.include_ignored, start),
            },
        }
    }
}

pub struct PathMatchCandidateSetIter<'a> {
    traversal: Traversal<'a>,
}

impl<'a> Iterator for PathMatchCandidateSetIter<'a> {
    type Item = fuzzy::PathMatchCandidate<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.traversal
            .next()
            .map(|entry| fuzzy::PathMatchCandidate {
                is_dir: entry.kind.is_dir(),
                path: &entry.path,
                char_bag: entry.char_bag,
            })
    }
}

impl<'a> fuzzy_nucleo::PathMatchCandidateSet<'a> for PathMatchCandidateSet {
    type Candidates = PathMatchCandidateSetNucleoIter<'a>;
    fn id(&self) -> usize {
        self.snapshot.id().to_usize()
    }
    fn len(&self) -> usize {
        match self.candidates {
            Candidates::Files => {
                if self.include_ignored {
                    self.snapshot.file_count()
                } else {
                    self.snapshot.visible_file_count()
                }
            }
            Candidates::Directories => {
                if self.include_ignored {
                    self.snapshot.dir_count()
                } else {
                    self.snapshot.visible_dir_count()
                }
            }
            Candidates::Entries => {
                if self.include_ignored {
                    self.snapshot.entry_count()
                } else {
                    self.snapshot.visible_entry_count()
                }
            }
        }
    }
    fn prefix(&self) -> Arc<RelPath> {
        if self.snapshot.root_entry().is_some_and(|e| e.is_file()) || self.include_root_name {
            self.snapshot.root_name().into()
        } else {
            RelPath::empty().into()
        }
    }
    fn root_is_file(&self) -> bool {
        self.snapshot.root_entry().is_some_and(|f| f.is_file())
    }
    fn path_style(&self) -> PathStyle {
        self.snapshot.path_style()
    }
    fn candidates(&'a self, start: usize) -> Self::Candidates {
        PathMatchCandidateSetNucleoIter {
            traversal: match self.candidates {
                Candidates::Directories => self.snapshot.directories(self.include_ignored, start),
                Candidates::Files => self.snapshot.files(self.include_ignored, start),
                Candidates::Entries => self.snapshot.entries(self.include_ignored, start),
            },
        }
    }
}

pub struct PathMatchCandidateSetNucleoIter<'a> {
    traversal: Traversal<'a>,
}

impl<'a> Iterator for PathMatchCandidateSetNucleoIter<'a> {
    type Item = fuzzy_nucleo::PathMatchCandidate<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        self.traversal
            .next()
            .map(|entry| fuzzy_nucleo::PathMatchCandidate {
                is_dir: entry.kind.is_dir(),
                path: &entry.path,
                char_bag: entry.char_bag,
            })
    }
}

impl EventEmitter<Event> for Project {}

impl<'a> From<&'a ProjectPath> for SettingsLocation<'a> {
    fn from(val: &'a ProjectPath) -> Self {
        SettingsLocation {
            worktree_id: val.worktree_id,
            path: val.path.as_ref(),
        }
    }
}

impl<P: Into<Arc<RelPath>>> From<(WorktreeId, P)> for ProjectPath {
    fn from((worktree_id, path): (WorktreeId, P)) -> Self {
        Self {
            worktree_id,
            path: path.into(),
        }
    }
}

/// ResolvedPath is a path that has been resolved to either a ProjectPath
/// or an AbsPath and that *exists*.
#[derive(Debug, Clone)]
pub enum ResolvedPath {
    ProjectPath {
        project_path: ProjectPath,
        is_dir: bool,
    },
    AbsPath {
        path: String,
        is_dir: bool,
    },
}

impl ResolvedPath {
    pub fn abs_path(&self) -> Option<&str> {
        match self {
            Self::AbsPath { path, .. } => Some(path),
            _ => None,
        }
    }

    pub fn into_abs_path(self) -> Option<String> {
        match self {
            Self::AbsPath { path, .. } => Some(path),
            _ => None,
        }
    }

    pub fn project_path(&self) -> Option<&ProjectPath> {
        match self {
            Self::ProjectPath { project_path, .. } => Some(project_path),
            _ => None,
        }
    }

    pub fn is_file(&self) -> bool {
        !self.is_dir()
    }

    pub fn is_dir(&self) -> bool {
        match self {
            Self::ProjectPath { is_dir, .. } => *is_dir,
            Self::AbsPath { is_dir, .. } => *is_dir,
        }
    }
}

impl ProjectItem for Buffer {
    fn try_open(
        project: &Entity<Project>,
        path: &ProjectPath,
        cx: &mut App,
    ) -> Option<Task<Result<Entity<Self>>>> {
        Some(project.update(cx, |project, cx| project.open_buffer(path.clone(), cx)))
    }

    fn entry_id(&self, _cx: &App) -> Option<ProjectEntryId> {
        File::from_dyn(self.file()).and_then(|file| file.project_entry_id())
    }

    fn project_path(&self, cx: &App) -> Option<ProjectPath> {
        let file = self.file()?;

        (!matches!(file.disk_state(), DiskState::Historic { .. })).then(|| ProjectPath {
            worktree_id: file.worktree_id(cx),
            path: file.path().clone(),
        })
    }

    fn is_dirty(&self) -> bool {
        self.is_dirty()
    }
}
