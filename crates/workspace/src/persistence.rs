pub mod model;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use anyhow::{Context as _, Result};
use chrono::{DateTime, Utc};
use collections::{HashMap, HashSet};
use fs::Fs;
use gpui::{Axis, Bounds, WindowBounds, WindowId, point, size};
use language::Toolchain;
use parking_lot::Mutex;
use project::{
    remote::{RemoteConnectionIdentity, RemoteConnectionOptions, remote_connection_identity},
    trusted_worktrees::{DbTrustedPaths, RemoteHostLocation},
};
use serde::{Deserialize, Serialize};
use ui::{App, px};
use util::rel_path::RelPath;
use uuid::Uuid;

use crate::{
    WorkspaceId,
    path_list::{PathList, SerializedPathList},
    persistence::model::{RemoteConnectionId, SerializedWorkspace},
};

use self::model::{
    DockStructure, SerializedPane, SerializedPaneGroup, SerializedWorkspaceLocation,
    SessionWorkspace,
};

const WORKSPACE_HISTORY_FILE: &str = "workspaces.json";
const MAX_STORED_WORKSPACES: usize = 100;

static WORKSPACE_DB_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct SerializedAxis(pub(crate) Axis);

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub(crate) struct SerializedWindowBounds(pub(crate) WindowBounds);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WindowBoundsJson {
    Windowed {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    Maximized {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    Fullscreen {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
}

impl From<WindowBounds> for WindowBoundsJson {
    fn from(bounds: WindowBounds) -> Self {
        let (kind, bounds) = match bounds {
            WindowBounds::Windowed(bounds) => ("windowed", bounds),
            WindowBounds::Maximized(bounds) => ("maximized", bounds),
            WindowBounds::Fullscreen(bounds) => ("fullscreen", bounds),
        };
        let origin = bounds.origin;
        let size = bounds.size;
        match kind {
            "windowed" => WindowBoundsJson::Windowed {
                x: f32::from(origin.x).round() as i32,
                y: f32::from(origin.y).round() as i32,
                width: f32::from(size.width).round() as i32,
                height: f32::from(size.height).round() as i32,
            },
            "maximized" => WindowBoundsJson::Maximized {
                x: f32::from(origin.x).round() as i32,
                y: f32::from(origin.y).round() as i32,
                width: f32::from(size.width).round() as i32,
                height: f32::from(size.height).round() as i32,
            },
            _ => WindowBoundsJson::Fullscreen {
                x: f32::from(origin.x).round() as i32,
                y: f32::from(origin.y).round() as i32,
                width: f32::from(size.width).round() as i32,
                height: f32::from(size.height).round() as i32,
            },
        }
    }
}

impl From<WindowBoundsJson> for WindowBounds {
    fn from(bounds: WindowBoundsJson) -> Self {
        let (kind, x, y, width, height) = match bounds {
            WindowBoundsJson::Windowed {
                x,
                y,
                width,
                height,
            } => ("windowed", x, y, width, height),
            WindowBoundsJson::Maximized {
                x,
                y,
                width,
                height,
            } => ("maximized", x, y, width, height),
            WindowBoundsJson::Fullscreen {
                x,
                y,
                width,
                height,
            } => ("fullscreen", x, y, width, height),
        };
        let bounds = Bounds {
            origin: point(px(x as f32), px(y as f32)),
            size: size(px(width as f32), px(height as f32)),
        };
        match kind {
            "windowed" => WindowBounds::Windowed(bounds),
            "maximized" => WindowBounds::Maximized(bounds),
            _ => WindowBounds::Fullscreen(bounds),
        }
    }
}

pub fn read_default_window_bounds() -> Option<(Uuid, WindowBounds)> {
    None
}

pub async fn write_default_window_bounds(_bounds: WindowBounds, _display_uuid: Uuid) -> Result<()> {
    Ok(())
}

pub async fn write_multi_workspace_state(_window_id: WindowId, _state: model::MultiWorkspaceState) {
}

pub fn read_serialized_multi_workspaces(
    session_workspaces: Vec<model::SessionWorkspace>,
    _cx: &App,
) -> Vec<model::SerializedMultiWorkspace> {
    let mut window_groups: Vec<Vec<model::SessionWorkspace>> = Vec::new();
    let mut window_id_to_group: HashMap<WindowId, usize> = HashMap::default();

    for session_workspace in session_workspaces {
        match session_workspace.window_id {
            Some(window_id) => {
                let group_index = *window_id_to_group.entry(window_id).or_insert_with(|| {
                    window_groups.push(Vec::new());
                    window_groups.len() - 1
                });
                window_groups[group_index].push(session_workspace);
            }
            None => {
                window_groups.push(vec![session_workspace]);
            }
        }
    }

    window_groups
        .into_iter()
        .filter_map(|mut group| {
            let active_workspace = group.drain(..).next()?;
            Some(model::SerializedMultiWorkspace {
                active_workspace,
                state: model::MultiWorkspaceState::default(),
            })
        })
        .collect()
}

pub fn read_default_dock_state() -> Option<DockStructure> {
    None
}

pub async fn write_default_dock_state(_docks: DockStructure) -> Result<()> {
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredWorkspaceDb {
    #[serde(default = "default_next_workspace_id")]
    next_workspace_id: i64,
    #[serde(default)]
    workspaces: Vec<StoredWorkspace>,
}

impl Default for StoredWorkspaceDb {
    fn default() -> Self {
        Self {
            next_workspace_id: default_next_workspace_id(),
            workspaces: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredWorkspace {
    id: WorkspaceId,
    location: SerializedWorkspaceLocation,
    paths: SerializedPathList,
    timestamp: DateTime<Utc>,
    #[serde(default)]
    centered_layout: bool,
    #[serde(default)]
    window_bounds: Option<WindowBoundsJson>,
    #[serde(default)]
    display: Option<Uuid>,
    #[serde(default)]
    docks: DockStructure,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    window_id: Option<u64>,
}

fn default_next_workspace_id() -> i64 {
    1
}

fn remote_connection_id_for_options(options: &RemoteConnectionOptions) -> RemoteConnectionId {
    match remote_connection_identity(options) {
        RemoteConnectionIdentity::Ssh { .. } => RemoteConnectionId(1),
        RemoteConnectionIdentity::Wsl { .. } => RemoteConnectionId(2),
        RemoteConnectionIdentity::Docker { .. } => RemoteConnectionId(3),
        #[cfg(any(test, feature = "test-support"))]
        RemoteConnectionIdentity::Mock { id } => RemoteConnectionId(id),
    }
}

fn workspace_history_path() -> PathBuf {
    paths::database_dir().join(WORKSPACE_HISTORY_FILE)
}

fn load_workspace_db() -> Result<StoredWorkspaceDb> {
    let path = workspace_history_path();
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StoredWorkspaceDb::default());
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading workspace history from {}", path.display()));
        }
    };

    if contents.trim().is_empty() {
        return Ok(StoredWorkspaceDb::default());
    }

    serde_json::from_str(&contents)
        .with_context(|| format!("parsing workspace history from {}", path.display()))
}

fn save_workspace_db(db: &StoredWorkspaceDb) -> Result<()> {
    let path = workspace_history_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("creating workspace history directory {}", parent.display())
        })?;
    }

    let contents = serde_json::to_string_pretty(db).context("serializing workspace history")?;
    std::fs::write(&path, contents)
        .with_context(|| format!("writing workspace history to {}", path.display()))
}

fn update_workspace_db<T>(update: impl FnOnce(&mut StoredWorkspaceDb) -> T) -> Result<T> {
    let _lock = WORKSPACE_DB_LOCK.lock();
    let mut db = load_workspace_db()?;
    let result = update(&mut db);
    prune_workspace_db(&mut db);
    save_workspace_db(&db)?;
    Ok(result)
}

fn prune_workspace_db(db: &mut StoredWorkspaceDb) {
    db.workspaces
        .sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    db.workspaces.truncate(MAX_STORED_WORKSPACES);

    let next_from_existing = db
        .workspaces
        .iter()
        .map(|workspace| i64::from(workspace.id) + 1)
        .max()
        .unwrap_or(default_next_workspace_id());
    db.next_workspace_id = db.next_workspace_id.max(next_from_existing);
}

impl StoredWorkspace {
    fn from_serialized(workspace: SerializedWorkspace) -> Self {
        Self {
            id: workspace.id,
            location: workspace.location,
            paths: workspace.paths.serialize(),
            timestamp: Utc::now(),
            centered_layout: workspace.centered_layout,
            window_bounds: workspace
                .window_bounds
                .map(|bounds| WindowBoundsJson::from(bounds.0)),
            display: workspace.display,
            docks: workspace.docks,
            session_id: workspace.session_id,
            window_id: workspace.window_id,
        }
    }

    fn path_list(&self) -> PathList {
        PathList::deserialize(&self.paths)
    }

    fn to_workspace_entry(&self) -> WorkspaceEntry {
        (
            self.id,
            self.location.clone(),
            self.path_list(),
            self.timestamp,
        )
    }

    fn to_serialized_workspace(&self) -> SerializedWorkspace {
        SerializedWorkspace {
            id: self.id,
            location: self.location.clone(),
            paths: self.path_list(),
            center_group: SerializedPaneGroup::Pane(SerializedPane::new(Vec::new(), true, 0)),
            window_bounds: self
                .window_bounds
                .clone()
                .map(|bounds| SerializedWindowBounds(WindowBounds::from(bounds))),
            centered_layout: self.centered_layout,
            display: self.display,
            docks: self.docks.clone(),
            session_id: self.session_id.clone(),
            bookmarks: Default::default(),
            user_toolchains: Default::default(),
            window_id: self.window_id,
        }
    }
}

fn workspaces_match(
    workspace: &StoredWorkspace,
    location: &SerializedWorkspaceLocation,
    paths: &PathList,
) -> bool {
    &workspace.location == location && workspace.path_list() == *paths
}

async fn local_paths_exist(paths: &PathList, fs: &dyn Fs) -> bool {
    if paths.is_empty() {
        return false;
    }

    for path in paths.paths() {
        if fs.metadata(path).await.ok().flatten().is_none() {
            return false;
        }
    }

    true
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WorkspaceDb;

impl WorkspaceDb {
    pub fn global(_cx: &App) -> Self {
        Self
    }

    #[cfg(any(test, feature = "test-support"))]
    pub async fn open_test_db(_name: &str) -> Self {
        Self
    }

    pub(crate) fn workspace_for_roots<P: AsRef<Path>>(
        &self,
        worktree_roots: &[P],
    ) -> Option<SerializedWorkspace> {
        let paths = PathList::new(worktree_roots);
        if paths.is_empty() {
            return None;
        }

        let _lock = WORKSPACE_DB_LOCK.lock();
        let db = match load_workspace_db() {
            Ok(db) => db,
            Err(error) => {
                log::error!("failed to load workspace history: {error:#}");
                return None;
            }
        };

        db.workspaces
            .iter()
            .find(|workspace| {
                workspaces_match(workspace, &SerializedWorkspaceLocation::Local, &paths)
            })
            .map(StoredWorkspace::to_serialized_workspace)
    }

    pub(crate) fn remote_workspace_for_roots<P: AsRef<Path>>(
        &self,
        worktree_roots: &[P],
        remote_project_id: RemoteConnectionId,
    ) -> Option<SerializedWorkspace> {
        let paths = PathList::new(worktree_roots);
        if paths.is_empty() {
            return None;
        }

        let _lock = WORKSPACE_DB_LOCK.lock();
        let db = match load_workspace_db() {
            Ok(db) => db,
            Err(error) => {
                log::error!("failed to load workspace history: {error:#}");
                return None;
            }
        };

        db.workspaces
            .iter()
            .find(|workspace| {
                let SerializedWorkspaceLocation::Remote(options) = &workspace.location else {
                    return false;
                };

                remote_connection_id_for_options(options) == remote_project_id
                    && workspace.path_list() == paths
            })
            .map(StoredWorkspace::to_serialized_workspace)
    }

    pub(crate) fn workspace_for_id(
        &self,
        workspace_id: WorkspaceId,
    ) -> Option<SerializedWorkspace> {
        let _lock = WORKSPACE_DB_LOCK.lock();
        let db = match load_workspace_db() {
            Ok(db) => db,
            Err(error) => {
                log::error!("failed to load workspace history: {error:#}");
                return None;
            }
        };

        db.workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .map(StoredWorkspace::to_serialized_workspace)
    }

    pub(crate) async fn save_workspace(&self, workspace: SerializedWorkspace) {
        let stored_workspace = StoredWorkspace::from_serialized(workspace);
        let stored_paths = stored_workspace.path_list();
        if let Err(error) = update_workspace_db(|db| {
            let existing = db.workspaces.iter().position(|workspace| {
                workspace.id == stored_workspace.id
                    || (!stored_paths.is_empty()
                        && workspaces_match(workspace, &stored_workspace.location, &stored_paths))
            });

            if let Some(index) = existing {
                db.workspaces[index] = stored_workspace;
            } else {
                db.workspaces.push(stored_workspace);
            }
        }) {
            log::error!("failed to save workspace history: {error:#}");
        }
    }

    pub(crate) async fn get_or_create_remote_connection(
        &self,
        options: RemoteConnectionOptions,
    ) -> Result<RemoteConnectionId> {
        Ok(remote_connection_id_for_options(&options))
    }

    pub async fn next_id(&self) -> Result<WorkspaceId> {
        update_workspace_db(|db| {
            let next_id = db.next_workspace_id.max(default_next_workspace_id());
            db.next_workspace_id = next_id + 1;
            WorkspaceId::from_i64(next_id)
        })
    }

    pub async fn delete_workspace_by_id(&self, id: WorkspaceId) -> Result<()> {
        update_workspace_db(|db| {
            db.workspaces.retain(|workspace| workspace.id != id);
        })
    }

    pub async fn recent_project_workspaces(
        &self,
        fs: &dyn Fs,
    ) -> Result<
        Vec<(
            WorkspaceId,
            SerializedWorkspaceLocation,
            PathList,
            DateTime<Utc>,
        )>,
    > {
        let workspaces = {
            let _lock = WORKSPACE_DB_LOCK.lock();
            load_workspace_db()?
                .workspaces
                .into_iter()
                .filter(|workspace| !workspace.path_list().is_empty())
                .map(|workspace| workspace.to_workspace_entry())
                .collect::<Vec<_>>()
        };

        let workspaces = resolve_worktree_workspaces(workspaces, fs).await;
        let mut existing_workspaces = Vec::new();

        for workspace in workspaces {
            if matches!(workspace.1, SerializedWorkspaceLocation::Local)
                && !local_paths_exist(&workspace.2, fs).await
            {
                continue;
            }
            existing_workspaces.push(workspace);
        }

        existing_workspaces.sort_by(|left, right| right.3.cmp(&left.3));
        Ok(existing_workspaces)
    }

    pub async fn garbage_collect_workspaces(
        &self,
        fs: &dyn Fs,
        current_session_id: &str,
        last_session_id: Option<&str>,
    ) -> Result<()> {
        let recent_workspace_ids = self
            .recent_project_workspaces(fs)
            .await?
            .into_iter()
            .map(|(id, _, _, _)| id)
            .collect::<HashSet<_>>();

        update_workspace_db(|db| {
            db.workspaces.retain(|workspace| {
                recent_workspace_ids.contains(&workspace.id)
                    || workspace.session_id.as_deref().is_some_and(|session_id| {
                        session_id == current_session_id || last_session_id == Some(session_id)
                    })
            });
        })
    }

    pub async fn last_workspace(
        &self,
        fs: &dyn Fs,
    ) -> Result<
        Option<(
            WorkspaceId,
            SerializedWorkspaceLocation,
            PathList,
            DateTime<Utc>,
        )>,
    > {
        Ok(self.recent_project_workspaces(fs).await?.into_iter().next())
    }

    pub async fn last_session_workspace_locations(
        &self,
        last_session_id: &str,
        last_session_window_stack: Option<Vec<WindowId>>,
        fs: &dyn Fs,
    ) -> Result<Vec<SessionWorkspace>> {
        let db = {
            let _lock = WORKSPACE_DB_LOCK.lock();
            load_workspace_db()?
        };

        let mut session_workspaces = Vec::new();
        for workspace in db.workspaces {
            if workspace.session_id.as_deref() != Some(last_session_id) {
                continue;
            }

            let paths = workspace.path_list();
            if matches!(workspace.location, SerializedWorkspaceLocation::Local)
                && !paths.is_empty()
                && !local_paths_exist(&paths, fs).await
            {
                continue;
            }

            session_workspaces.push(SessionWorkspace {
                workspace_id: workspace.id,
                location: workspace.location,
                paths,
                window_id: workspace.window_id.map(WindowId::from),
            });
        }

        if let Some(window_stack) = last_session_window_stack {
            session_workspaces.sort_by_key(|workspace| {
                workspace
                    .window_id
                    .and_then(|window_id| window_stack.iter().position(|id| *id == window_id))
                    .unwrap_or(window_stack.len())
            });
        }

        Ok(session_workspaces)
    }

    pub async fn update_timestamp(&self, workspace_id: WorkspaceId) -> Result<()> {
        update_workspace_db(|db| {
            if let Some(workspace) = db
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.id == workspace_id)
            {
                workspace.timestamp = Utc::now();
            }
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) async fn set_timestamp_for_tests(
        &self,
        workspace_id: WorkspaceId,
        timestamp: String,
    ) -> Result<()> {
        let timestamp = DateTime::parse_from_rfc3339(&timestamp)
            .with_context(|| format!("parsing timestamp {timestamp:?}"))?
            .with_timezone(&Utc);
        update_workspace_db(|db| {
            if let Some(workspace) = db
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.id == workspace_id)
            {
                workspace.timestamp = timestamp;
            }
        })
    }

    pub(crate) async fn set_window_open_status(
        &self,
        workspace_id: WorkspaceId,
        bounds: SerializedWindowBounds,
        display: Uuid,
    ) -> Result<()> {
        update_workspace_db(|db| {
            if let Some(workspace) = db
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.id == workspace_id)
            {
                workspace.window_bounds = Some(WindowBoundsJson::from(bounds.0));
                workspace.display = Some(display);
                workspace.timestamp = Utc::now();
            }
        })
    }

    pub(crate) async fn set_centered_layout(
        &self,
        workspace_id: WorkspaceId,
        centered_layout: bool,
    ) -> Result<()> {
        update_workspace_db(|db| {
            if let Some(workspace) = db
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.id == workspace_id)
            {
                workspace.centered_layout = centered_layout;
            }
        })
    }

    pub(crate) async fn set_session_id(
        &self,
        workspace_id: WorkspaceId,
        session_id: Option<String>,
    ) -> Result<()> {
        update_workspace_db(|db| {
            if let Some(workspace) = db
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.id == workspace_id)
            {
                workspace.session_id = session_id;
            }
        })
    }

    pub(crate) async fn set_session_binding(
        &self,
        workspace_id: WorkspaceId,
        session_id: Option<String>,
        window_id: Option<u64>,
    ) -> Result<()> {
        update_workspace_db(|db| {
            if let Some(workspace) = db
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.id == workspace_id)
            {
                workspace.session_id = session_id;
                workspace.window_id = window_id;
            }
        })
    }

    pub(crate) async fn toolchains(
        &self,
        _workspace_id: WorkspaceId,
    ) -> Result<Vec<(Toolchain, Arc<Path>, Arc<RelPath>)>> {
        Ok(Vec::new())
    }

    pub async fn set_toolchain(
        &self,
        _workspace_id: WorkspaceId,
        _worktree_root_path: Arc<Path>,
        _relative_worktree_path: Arc<RelPath>,
        _toolchain: Toolchain,
    ) -> Result<()> {
        Ok(())
    }

    pub(crate) async fn save_trusted_worktrees(
        &self,
        _trusted_worktrees: HashMap<Option<RemoteHostLocation>, HashSet<PathBuf>>,
    ) -> Result<()> {
        Ok(())
    }

    pub fn fetch_trusted_worktrees(&self) -> Result<DbTrustedPaths> {
        Ok(HashMap::default())
    }

    pub async fn clear_trusted_worktrees(&self) -> Result<()> {
        Ok(())
    }
}

type WorkspaceEntry = (
    WorkspaceId,
    SerializedWorkspaceLocation,
    PathList,
    DateTime<Utc>,
);

pub async fn resolve_worktree_workspaces(
    workspaces: impl IntoIterator<Item = WorkspaceEntry>,
    fs: &dyn Fs,
) -> Vec<WorkspaceEntry> {
    let resolved = futures::future::join_all(workspaces.into_iter().map(|entry| async move {
        let paths = entry.2.paths();
        if paths.is_empty() {
            return entry;
        }

        let resolved_paths = futures::future::join_all(
            paths
                .iter()
                .map(|path| project::git_store::resolve_git_worktree_to_main_repo(fs, path)),
        )
        .await;

        if resolved_paths.iter().all(|resolved| resolved.is_none()) {
            return entry;
        }

        let new_paths = paths
            .iter()
            .zip(resolved_paths.iter())
            .map(|(original, resolved)| {
                resolved
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| original.clone())
            })
            .collect::<Vec<_>>();

        let new_path_refs = new_paths
            .iter()
            .map(|path| path.as_path())
            .collect::<Vec<_>>();
        (entry.0, entry.1, PathList::new(&new_path_refs), entry.3)
    }))
    .await;

    let mut seen: HashMap<Vec<PathBuf>, usize> = HashMap::default();
    let mut result: Vec<WorkspaceEntry> = Vec::new();

    for entry in resolved {
        let key = entry.2.paths().to_vec();
        if let Some(&existing_index) = seen.get(&key) {
            if entry.3 > result[existing_index].3 {
                result[existing_index] = entry;
            }
        } else {
            seen.insert(key, result.len());
            result.push(entry);
        }
    }

    result
}
