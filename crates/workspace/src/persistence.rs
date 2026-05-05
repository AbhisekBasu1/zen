pub mod model;

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use collections::{HashMap, HashSet};
use fs::Fs;
use gpui::{Axis, Bounds, WindowBounds, WindowId, point, size};
use language::Toolchain;
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
    path_list::PathList,
    persistence::model::{RemoteConnectionId, SerializedWorkspace},
};

use self::model::{DockStructure, SerializedWorkspaceLocation, SessionWorkspace};

static NEXT_WORKSPACE_ID: AtomicI64 = AtomicI64::new(1);

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct SerializedAxis(pub(crate) Axis);

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub(crate) struct SerializedWindowBounds(pub(crate) WindowBounds);

#[derive(Serialize, Deserialize)]
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
        _worktree_roots: &[P],
    ) -> Option<SerializedWorkspace> {
        None
    }

    pub(crate) fn remote_workspace_for_roots<P: AsRef<Path>>(
        &self,
        _worktree_roots: &[P],
        _remote_project_id: RemoteConnectionId,
    ) -> Option<SerializedWorkspace> {
        None
    }

    pub(crate) fn workspace_for_id(
        &self,
        _workspace_id: WorkspaceId,
    ) -> Option<SerializedWorkspace> {
        None
    }

    pub(crate) async fn save_workspace(&self, _workspace: SerializedWorkspace) {}

    pub(crate) async fn get_or_create_remote_connection(
        &self,
        options: RemoteConnectionOptions,
    ) -> Result<RemoteConnectionId> {
        let identity = remote_connection_identity(&options);
        let id = match identity {
            RemoteConnectionIdentity::Ssh { .. } => 1,
            RemoteConnectionIdentity::Wsl { .. } => 2,
            RemoteConnectionIdentity::Docker { .. } => 3,
            #[cfg(any(test, feature = "test-support"))]
            RemoteConnectionIdentity::Mock { id } => id,
        };
        Ok(RemoteConnectionId(id))
    }

    pub async fn next_id(&self) -> Result<WorkspaceId> {
        Ok(WorkspaceId::from_i64(
            NEXT_WORKSPACE_ID.fetch_add(1, Ordering::Relaxed),
        ))
    }

    pub async fn delete_workspace_by_id(&self, _id: WorkspaceId) -> Result<()> {
        Ok(())
    }

    pub async fn recent_project_workspaces(
        &self,
        _fs: &dyn Fs,
    ) -> Result<
        Vec<(
            WorkspaceId,
            SerializedWorkspaceLocation,
            PathList,
            DateTime<Utc>,
        )>,
    > {
        Ok(Vec::new())
    }

    pub async fn garbage_collect_workspaces(
        &self,
        _fs: &dyn Fs,
        _current_session_id: &str,
        _last_session_id: Option<&str>,
    ) -> Result<()> {
        Ok(())
    }

    pub async fn last_workspace(
        &self,
        _fs: &dyn Fs,
    ) -> Result<
        Option<(
            WorkspaceId,
            SerializedWorkspaceLocation,
            PathList,
            DateTime<Utc>,
        )>,
    > {
        Ok(None)
    }

    pub async fn last_session_workspace_locations(
        &self,
        _last_session_id: &str,
        _last_session_window_stack: Option<Vec<WindowId>>,
        _fs: &dyn Fs,
    ) -> Result<Vec<SessionWorkspace>> {
        Ok(Vec::new())
    }

    pub async fn update_timestamp(&self, _workspace_id: WorkspaceId) -> Result<()> {
        Ok(())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) async fn set_timestamp_for_tests(
        &self,
        _workspace_id: WorkspaceId,
        _timestamp: String,
    ) -> Result<()> {
        Ok(())
    }

    pub(crate) async fn set_window_open_status(
        &self,
        _workspace_id: WorkspaceId,
        _bounds: SerializedWindowBounds,
        _display: Uuid,
    ) -> Result<()> {
        Ok(())
    }

    pub(crate) async fn set_centered_layout(
        &self,
        _workspace_id: WorkspaceId,
        _centered_layout: bool,
    ) -> Result<()> {
        Ok(())
    }

    pub(crate) async fn set_session_id(
        &self,
        _workspace_id: WorkspaceId,
        _session_id: Option<String>,
    ) -> Result<()> {
        Ok(())
    }

    pub(crate) async fn set_session_binding(
        &self,
        _workspace_id: WorkspaceId,
        _session_id: Option<String>,
        _window_id: Option<u64>,
    ) -> Result<()> {
        Ok(())
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
