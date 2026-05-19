use collections::HashMap;
use fs::Fs;
use gpui::{BorrowAppContext, Context, Entity, EventEmitter, Subscription};
use paths::{EDITORCONFIG_NAME, local_settings_file_relative_path};
use settings::{
    EditorconfigEvent, InvalidSettingsError, LocalSettingsKind, LocalSettingsPath, RegisterSetting,
    Settings, SettingsStore,
};
use std::{cell::OnceCell, collections::BTreeMap, path::PathBuf, sync::Arc};
use util::{ResultExt, rel_path::RelPath};
use worktree::{PathChange, UpdatedEntriesSet, Worktree, WorktreeId};

use crate::{
    trusted_worktrees::{PathTrust, TrustedWorktrees, TrustedWorktreesEvent},
    worktree_store::{WorktreeStore, WorktreeStoreEvent},
};

#[derive(Debug, Clone, RegisterSetting)]
pub struct ProjectSettings {
    /// Configuration for session-related features
    pub session: SessionSettings,
}

#[derive(Copy, Clone, Debug)]
pub struct SessionSettings {
    /// Whether or not to restore unsaved buffers on restart.
    ///
    /// If this is true, user won't be prompted whether to save/discard
    /// dirty files when closing the application.
    ///
    /// Default: true
    pub restore_unsaved_buffers: bool,
    /// Whether or not to skip worktree trust checks.
    /// When trusted, project settings are synchronized automatically,
    /// language and MCP servers are downloaded and started automatically.
    ///
    /// Default: false
    pub trust_all_worktrees: bool,
}

impl Settings for ProjectSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        Self {
            session: SessionSettings {
                restore_unsaved_buffers: content.session.unwrap().restore_unsaved_buffers.unwrap(),
                trust_all_worktrees: content.session.unwrap().trust_all_worktrees.unwrap(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsObserverEvent {
    LocalSettingsUpdated(Result<PathBuf, InvalidSettingsError>),
}

impl EventEmitter<SettingsObserverEvent> for SettingsObserver {}

pub struct SettingsObserver {
    fs: Arc<dyn Fs>,
    worktree_store: Entity<WorktreeStore>,
    pending_local_settings:
        HashMap<PathTrust, BTreeMap<(WorktreeId, Arc<RelPath>), Option<String>>>,
    _trusted_worktrees_watcher: Option<Subscription>,
    _editorconfig_watcher: Option<Subscription>,
}

/// SettingsObserver observes changes to .zed/settings.json files in local worktrees
/// and updates local settings.
impl SettingsObserver {
    pub fn new_local(
        fs: Arc<dyn Fs>,
        worktree_store: Entity<WorktreeStore>,
        _watch_global_configs: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&worktree_store, Self::on_worktree_store_event)
            .detach();

        let _trusted_worktrees_watcher =
            TrustedWorktrees::try_get_global(cx).map(|trusted_worktrees| {
                cx.subscribe(
                    &trusted_worktrees,
                    move |settings_observer, _, e, cx| match e {
                        TrustedWorktreesEvent::Trusted(_, trusted_paths) => {
                            for trusted_path in trusted_paths {
                                if let Some(pending_local_settings) = settings_observer
                                    .pending_local_settings
                                    .remove(trusted_path)
                                {
                                    for ((worktree_id, directory_path), settings_contents) in
                                        pending_local_settings
                                    {
                                        let path =
                                            LocalSettingsPath::InWorktree(directory_path.clone());
                                        apply_local_settings(
                                            worktree_id,
                                            path.clone(),
                                            LocalSettingsKind::Settings,
                                            &settings_contents,
                                            cx,
                                        );
                                    }
                                }
                            }
                        }
                        TrustedWorktreesEvent::Restricted(..) => {}
                    },
                )
            });

        let editorconfig_store = cx.global::<SettingsStore>().editorconfig_store.clone();
        let _editorconfig_watcher = cx.subscribe(
            &editorconfig_store,
            |this, _, event: &EditorconfigEvent, cx| {
                let EditorconfigEvent::ExternalConfigChanged {
                    path,
                    content,
                    affected_worktree_ids,
                } = event;
                for worktree_id in affected_worktree_ids {
                    if let Some(worktree) = this
                        .worktree_store
                        .read(cx)
                        .worktree_for_id(*worktree_id, cx)
                    {
                        this.update_settings(
                            worktree,
                            [(
                                path.clone(),
                                LocalSettingsKind::Editorconfig,
                                content.clone(),
                            )],
                            cx,
                        );
                    }
                }
            },
        );

        Self {
            worktree_store,
            fs,
            _trusted_worktrees_watcher,
            pending_local_settings: HashMap::default(),
            _editorconfig_watcher: Some(_editorconfig_watcher),
        }
    }

    fn on_worktree_store_event(
        &mut self,
        _: Entity<WorktreeStore>,
        event: &WorktreeStoreEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            WorktreeStoreEvent::WorktreeAdded(worktree) => cx
                .subscribe(worktree, |this, worktree, event, cx| {
                    if let worktree::Event::UpdatedEntries(changes) = event {
                        this.update_local_worktree_settings(&worktree, changes, cx)
                    }
                })
                .detach(),
            WorktreeStoreEvent::WorktreeRemoved(_, worktree_id) => {
                cx.update_global::<SettingsStore, _>(|store, cx| {
                    store.clear_local_settings(*worktree_id, cx).log_err();
                });
            }
            _ => {}
        }
    }

    fn update_local_worktree_settings(
        &mut self,
        worktree: &Entity<Worktree>,
        changes: &UpdatedEntriesSet,
        cx: &mut Context<Self>,
    ) {
        let mut settings_contents = Vec::new();
        for (path, _, change) in changes.iter() {
            let (settings_dir, kind) = if path.ends_with(local_settings_file_relative_path()) {
                let settings_dir = path
                    .ancestors()
                    .nth(local_settings_file_relative_path().components().count())
                    .unwrap()
                    .into();
                (settings_dir, LocalSettingsKind::Settings)
            } else if path.ends_with(RelPath::unix(EDITORCONFIG_NAME).unwrap()) {
                let Some(settings_dir) = path.parent().map(Arc::from) else {
                    continue;
                };
                if matches!(change, PathChange::Loaded) || matches!(change, PathChange::Added) {
                    let worktree_id = worktree.read(cx).id();
                    let worktree_path = worktree.read(cx).abs_path();
                    let fs = self.fs.clone();
                    cx.update_global::<SettingsStore, _>(|store, cx| {
                        store
                            .editorconfig_store
                            .update(cx, |editorconfig_store, cx| {
                                editorconfig_store.discover_local_external_configs_chain(
                                    worktree_id,
                                    worktree_path,
                                    fs,
                                    cx,
                                );
                            });
                    });
                }
                (settings_dir, LocalSettingsKind::Editorconfig)
            } else {
                continue;
            };

            let removed = change == &PathChange::Removed;
            let fs = self.fs.clone();
            let abs_path = worktree.read(cx).absolutize(path);
            settings_contents.push(async move {
                (
                    settings_dir,
                    kind,
                    if removed {
                        None
                    } else {
                        Some(
                            async move {
                                let content = fs.load(&abs_path).await?;
                                Ok::<_, anyhow::Error>(content)
                            }
                            .await,
                        )
                    },
                )
            });
        }

        if settings_contents.is_empty() {
            return;
        }

        let worktree = worktree.clone();
        cx.spawn(async move |this, cx| {
            let settings_contents: Vec<(Arc<RelPath>, _, _)> =
                futures::future::join_all(settings_contents).await;
            cx.update(|cx| {
                this.update(cx, |this, cx| {
                    this.update_settings(
                        worktree,
                        settings_contents.into_iter().map(|(path, kind, content)| {
                            (
                                LocalSettingsPath::InWorktree(path),
                                kind,
                                content.and_then(|c| c.log_err()),
                            )
                        }),
                        cx,
                    )
                })
            })
        })
        .detach();
    }

    fn update_settings(
        &mut self,
        worktree: Entity<Worktree>,
        settings_contents: impl IntoIterator<
            Item = (LocalSettingsPath, LocalSettingsKind, Option<String>),
        >,
        cx: &mut Context<Self>,
    ) {
        let worktree_id = worktree.read(cx).id();
        let can_trust_worktree = OnceCell::new();
        for (directory_path, kind, file_content) in settings_contents {
            match (&directory_path, kind) {
                (LocalSettingsPath::InWorktree(directory), LocalSettingsKind::Settings) => {
                    if *can_trust_worktree.get_or_init(|| {
                        if let Some(trusted_worktrees) = TrustedWorktrees::try_get_global(cx) {
                            trusted_worktrees.update(cx, |trusted_worktrees, cx| {
                                trusted_worktrees.can_trust(&self.worktree_store, worktree_id, cx)
                            })
                        } else {
                            true
                        }
                    }) {
                        apply_local_settings(
                            worktree_id,
                            LocalSettingsPath::InWorktree(directory.clone()),
                            kind,
                            &file_content,
                            cx,
                        )
                    } else {
                        self.pending_local_settings
                            .entry(PathTrust::Worktree(worktree_id))
                            .or_default()
                            .insert((worktree_id, directory.clone()), file_content.clone());
                    }
                }
                (directory, LocalSettingsKind::Editorconfig) => {
                    apply_local_settings(worktree_id, directory.clone(), kind, &file_content, cx);
                }
                (LocalSettingsPath::OutsideWorktree(path), kind) => {
                    log::error!(
                        "OutsideWorktree path {:?} with kind {:?} is only supported by editorconfig",
                        path,
                        kind
                    );
                    continue;
                }
            };
        }
    }
}

fn apply_local_settings(
    worktree_id: WorktreeId,
    path: LocalSettingsPath,
    kind: LocalSettingsKind,
    file_content: &Option<String>,
    cx: &mut Context<'_, SettingsObserver>,
) {
    cx.update_global::<SettingsStore, _>(|store, cx| {
        let result =
            store.set_local_settings(worktree_id, path.clone(), kind, file_content.as_deref(), cx);

        match result {
            Err(InvalidSettingsError::LocalSettings { path, message }) => {
                log::error!("Failed to set local settings in {path:?}: {message}");
                cx.emit(SettingsObserverEvent::LocalSettingsUpdated(Err(
                    InvalidSettingsError::LocalSettings { path, message },
                )));
            }
            Err(e) => log::error!("Failed to set local settings: {e}"),
            Ok(()) => {
                let settings_path = match &path {
                    LocalSettingsPath::InWorktree(rel_path) => rel_path
                        .as_std_path()
                        .join(local_settings_file_relative_path().as_std_path()),
                    LocalSettingsPath::OutsideWorktree(abs_path) => abs_path.to_path_buf(),
                };
                cx.emit(SettingsObserverEvent::LocalSettingsUpdated(Ok(
                    settings_path,
                )))
            }
        }
    })
}
