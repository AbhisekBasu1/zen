use crate::{
    ProjectPath,
    worktree_store::{WorktreeStore, WorktreeStoreEvent},
};
use anyhow::{Context as _, Result, anyhow};
use collections::{HashMap, HashSet, hash_map};
use futures::{Future, FutureExt as _, future::Shared};
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Subscription, Task, WeakEntity};
use language::{
    Buffer, BufferEvent, Capability, DiskState, File as _, Language, LineEnding,
    language_settings::{AllLanguageSettings, LineEndingSetting},
};

use settings::Settings;
use std::{io, sync::Arc};
use text::{BufferId, ReplicaId};
use util::{ResultExt as _, debug_panic, rel_path::RelPath};
use worktree::{File, PathChange, ProjectEntryId, Worktree, WorktreeSettings};

/// A set of open buffers.
pub struct BufferStore {
    local_store: LocalBufferStore,
    #[allow(clippy::type_complexity)]
    loading_buffers: HashMap<ProjectPath, Shared<Task<Result<Entity<Buffer>, Arc<anyhow::Error>>>>>,
    worktree_store: Entity<WorktreeStore>,
    opened_buffers: HashMap<BufferId, WeakEntity<Buffer>>,
    path_to_buffer_id: HashMap<ProjectPath, BufferId>,
}

struct LocalBufferStore {
    local_buffer_ids_by_entry_id: HashMap<ProjectEntryId, BufferId>,
    worktree_store: Entity<WorktreeStore>,
    _subscription: Subscription,
}

pub enum BufferStoreEvent {
    BufferAdded(Entity<Buffer>),
    BufferDropped(BufferId),
    BufferChangedFilePath {
        buffer: Entity<Buffer>,
        old_file: Option<Arc<dyn language::File>>,
    },
}

#[derive(Default, Debug, Clone)]
pub struct ProjectTransaction(pub HashMap<Entity<Buffer>, language::Transaction>);

impl PartialEq for ProjectTransaction {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().all(|(buffer, transaction)| {
                other.0.get(buffer).is_some_and(|t| t.id == transaction.id)
            })
    }
}

impl EventEmitter<BufferStoreEvent> for BufferStore {}

impl LocalBufferStore {
    fn save_local_buffer(
        &self,
        buffer_handle: Entity<Buffer>,
        worktree: Entity<Worktree>,
        path: Arc<RelPath>,
        mut has_changed_file: bool,
        cx: &mut Context<BufferStore>,
    ) -> Task<Result<()>> {
        let buffer = buffer_handle.read(cx);

        let text = buffer.as_rope().clone();
        let line_ending = buffer.line_ending();
        let encoding = buffer.encoding();
        let has_bom = buffer.has_bom();
        let version = buffer.version();
        let file = buffer.file().cloned();
        if file
            .as_ref()
            .is_some_and(|file| file.disk_state() == DiskState::New)
        {
            has_changed_file = true;
        }

        let save = worktree.update(cx, |worktree, cx| {
            worktree.write_file(path, text, line_ending, encoding, has_bom, cx)
        });

        cx.spawn(async move |_, cx| {
            let new_file = save.await?;
            let mtime = new_file.disk_state().mtime();
            buffer_handle.update(cx, |buffer, cx| {
                if has_changed_file {
                    buffer.file_updated(new_file, cx);
                }
                buffer.did_save(version.clone(), mtime, cx);
            });
            Ok(())
        })
    }

    fn subscribe_to_worktree(
        &mut self,
        worktree: &Entity<Worktree>,
        cx: &mut Context<BufferStore>,
    ) {
        cx.subscribe(worktree, |this, worktree, event, cx| {
            if worktree.read(cx).is_local()
                && let worktree::Event::UpdatedEntries(changes) = event
            {
                Self::local_worktree_entries_changed(this, &worktree, changes, cx);
            }
        })
        .detach();
    }

    fn local_worktree_entries_changed(
        this: &mut BufferStore,
        worktree_handle: &Entity<Worktree>,
        changes: &[(Arc<RelPath>, ProjectEntryId, PathChange)],
        cx: &mut Context<BufferStore>,
    ) {
        let snapshot = worktree_handle.read(cx).snapshot();
        for (path, entry_id, _) in changes {
            Self::local_worktree_entry_changed(
                this,
                *entry_id,
                path,
                worktree_handle,
                &snapshot,
                cx,
            );
        }
    }

    fn local_worktree_entry_changed(
        this: &mut BufferStore,
        entry_id: ProjectEntryId,
        path: &Arc<RelPath>,
        worktree: &Entity<worktree::Worktree>,
        snapshot: &worktree::Snapshot,
        cx: &mut Context<BufferStore>,
    ) -> Option<()> {
        let project_path = ProjectPath {
            worktree_id: snapshot.id(),
            path: path.clone(),
        };

        let buffer_id = this
            .local_store
            .local_buffer_ids_by_entry_id
            .get(&entry_id)
            .copied()
            .or_else(|| this.path_to_buffer_id.get(&project_path).copied())?;

        let buffer = if let Some(buffer) = this.get(buffer_id) {
            Some(buffer)
        } else {
            this.opened_buffers.remove(&buffer_id);
            None
        };

        let buffer = if let Some(buffer) = buffer {
            buffer
        } else {
            this.path_to_buffer_id.remove(&project_path);
            this.local_store
                .local_buffer_ids_by_entry_id
                .remove(&entry_id);
            return None;
        };

        let events = buffer.update(cx, |buffer, cx| {
            let file = buffer.file()?;
            let old_file = File::from_dyn(Some(file))?;
            if old_file.worktree != *worktree {
                return None;
            }

            let snapshot_entry = old_file
                .entry_id
                .and_then(|entry_id| snapshot.entry_for_id(entry_id))
                .or_else(|| snapshot.entry_for_path(old_file.path.as_ref()));

            let new_file = if let Some(entry) = snapshot_entry {
                File {
                    disk_state: match entry.mtime {
                        Some(mtime) => DiskState::Present {
                            mtime,
                            size: entry.size,
                        },
                        None => old_file.disk_state,
                    },
                    is_local: true,
                    entry_id: Some(entry.id),
                    path: entry.path.clone(),
                    worktree: worktree.clone(),
                    is_private: entry.is_private,
                }
            } else {
                File {
                    disk_state: DiskState::Deleted,
                    is_local: true,
                    entry_id: old_file.entry_id,
                    path: old_file.path.clone(),
                    worktree: worktree.clone(),
                    is_private: old_file.is_private,
                }
            };

            if new_file == *old_file {
                return None;
            }

            let mut events = Vec::new();
            if new_file.path != old_file.path {
                this.path_to_buffer_id.remove(&ProjectPath {
                    path: old_file.path.clone(),
                    worktree_id: old_file.worktree_id(cx),
                });
                this.path_to_buffer_id.insert(
                    ProjectPath {
                        worktree_id: new_file.worktree_id(cx),
                        path: new_file.path.clone(),
                    },
                    buffer_id,
                );
                events.push(BufferStoreEvent::BufferChangedFilePath {
                    buffer: cx.entity(),
                    old_file: buffer.file().cloned(),
                });
            }
            if new_file.entry_id != old_file.entry_id {
                if let Some(entry_id) = old_file.entry_id {
                    this.local_store
                        .local_buffer_ids_by_entry_id
                        .remove(&entry_id);
                }
                if let Some(entry_id) = new_file.entry_id {
                    this.local_store
                        .local_buffer_ids_by_entry_id
                        .insert(entry_id, buffer_id);
                }
            }

            buffer.file_updated(Arc::new(new_file), cx);
            Some(events)
        })?;

        for event in events {
            cx.emit(event);
        }

        None
    }

    fn save_buffer(
        &self,
        buffer: Entity<Buffer>,
        cx: &mut Context<BufferStore>,
    ) -> Task<Result<()>> {
        let Some(file) = File::from_dyn(buffer.read(cx).file()) else {
            return Task::ready(Err(anyhow!("buffer doesn't have a file")));
        };
        let worktree = file.worktree.clone();
        self.save_local_buffer(buffer, worktree, file.path.clone(), false, cx)
    }

    fn save_buffer_as(
        &self,
        buffer: Entity<Buffer>,
        path: ProjectPath,
        cx: &mut Context<BufferStore>,
    ) -> Task<Result<()>> {
        let Some(worktree) = self
            .worktree_store
            .read(cx)
            .worktree_for_id(path.worktree_id, cx)
        else {
            return Task::ready(Err(anyhow!("no such worktree")));
        };
        self.save_local_buffer(buffer, worktree, path.path, true, cx)
    }

    #[ztracing::instrument(skip_all)]
    fn open_buffer(
        &self,
        path: Arc<RelPath>,
        worktree: Entity<Worktree>,
        cx: &mut Context<BufferStore>,
    ) -> Task<Result<Entity<Buffer>>> {
        let load_file = worktree.update(cx, |worktree, cx| worktree.load_file(path.as_ref(), cx));
        cx.spawn(async move |this, cx| {
            let path = path.clone();
            let buffer = match load_file.await {
                Ok(loaded) => {
                    let reservation = cx.reserve_entity::<Buffer>();
                    let buffer_id = BufferId::from(reservation.entity_id().as_non_zero_u64());
                    let text_buffer = cx
                        .background_spawn(async move {
                            text::Buffer::new(ReplicaId::LOCAL, buffer_id, loaded.text)
                        })
                        .await;
                    cx.insert_entity(reservation, |_| {
                        let mut buffer =
                            Buffer::build(text_buffer, Some(loaded.file), Capability::ReadWrite);
                        buffer.set_encoding(loaded.encoding);
                        buffer.set_has_bom(loaded.has_bom);
                        buffer
                    })
                }
                Err(error) if is_not_found_error(&error) => cx.new(|cx| {
                    let buffer_id = BufferId::from(cx.entity_id().as_non_zero_u64());
                    let text_buffer = text::Buffer::new(ReplicaId::LOCAL, buffer_id, "");
                    let mut buffer = Buffer::build(
                        text_buffer,
                        Some(Arc::new(File {
                            worktree,
                            path,
                            disk_state: DiskState::New,
                            entry_id: None,
                            is_local: true,
                            is_private: false,
                        })),
                        Capability::ReadWrite,
                    );
                    apply_initial_line_ending(&mut buffer, cx);
                    buffer
                }),
                Err(e) => return Err(e),
            };
            this.update(cx, |this, cx| {
                this.add_buffer(buffer.clone(), cx)?;
                let buffer_id = buffer.read(cx).remote_id();
                if let Some(file) = File::from_dyn(buffer.read(cx).file()) {
                    let project_path = ProjectPath {
                        worktree_id: file.worktree_id(cx),
                        path: file.path.clone(),
                    };
                    let entry_id = file.entry_id;

                    // Check if the file should be read-only based on settings
                    let settings = WorktreeSettings::get(Some((&project_path).into()), cx);
                    let is_read_only = if project_path.path.is_empty() {
                        settings.is_std_path_read_only(&file.full_path(cx))
                    } else {
                        settings.is_path_read_only(&project_path.path)
                    };
                    if is_read_only {
                        buffer.update(cx, |buffer, cx| {
                            buffer.set_capability(Capability::Read, cx);
                        });
                    }

                    this.path_to_buffer_id.insert(project_path, buffer_id);
                    if let Some(entry_id) = entry_id {
                        this.local_store
                            .local_buffer_ids_by_entry_id
                            .insert(entry_id, buffer_id);
                    }
                }

                anyhow::Ok(())
            })??;

            Ok(buffer)
        })
    }

    fn create_buffer(
        &self,
        language: Option<Arc<Language>>,
        cx: &mut Context<BufferStore>,
    ) -> Task<Result<Entity<Buffer>>> {
        cx.spawn(async move |buffer_store, cx| {
            let buffer = cx.new(|cx| {
                let mut buffer = Buffer::local("", cx)
                    .with_language(language.unwrap_or_else(|| language::PLAIN_TEXT.clone()), cx);
                apply_initial_line_ending(&mut buffer, cx);
                buffer
            });
            buffer_store.update(cx, |buffer_store, cx| {
                buffer_store.add_buffer(buffer.clone(), cx).log_err();
            })?;
            Ok(buffer)
        })
    }

    fn reload_buffers(
        &self,
        buffers: HashSet<Entity<Buffer>>,
        push_to_history: bool,
        cx: &mut Context<BufferStore>,
    ) -> Task<Result<ProjectTransaction>> {
        cx.spawn(async move |_, cx| {
            let mut project_transaction = ProjectTransaction::default();
            for buffer in buffers {
                let transaction = buffer.update(cx, |buffer, cx| buffer.reload(cx)).await?;
                buffer.update(cx, |buffer, cx| {
                    if let Some(transaction) = transaction {
                        if !push_to_history {
                            buffer.forget_transaction(transaction.id);
                        }
                        project_transaction.0.insert(cx.entity(), transaction);
                    }
                });
            }

            Ok(project_transaction)
        })
    }
}

impl BufferStore {
    /// Creates a buffer store, optionally retaining its buffers.
    pub fn local(worktree_store: Entity<WorktreeStore>, cx: &mut Context<Self>) -> Self {
        Self {
            local_store: LocalBufferStore {
                local_buffer_ids_by_entry_id: Default::default(),
                worktree_store: worktree_store.clone(),
                _subscription: cx.subscribe(&worktree_store, |this, _, event, cx| {
                    if let WorktreeStoreEvent::WorktreeAdded(worktree) = event {
                        this.local_store.subscribe_to_worktree(worktree, cx);
                    }
                }),
            },
            opened_buffers: Default::default(),
            path_to_buffer_id: Default::default(),
            loading_buffers: Default::default(),
            worktree_store,
        }
    }

    #[ztracing::instrument(skip_all)]
    pub fn open_buffer(
        &mut self,
        project_path: ProjectPath,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Buffer>>> {
        if let Some(buffer) = self.get_by_path(&project_path) {
            return Task::ready(Ok(buffer));
        }

        let task = match self.loading_buffers.entry(project_path.clone()) {
            hash_map::Entry::Occupied(e) => e.get().clone(),
            hash_map::Entry::Vacant(entry) => {
                let path = project_path.path.clone();
                let Some(worktree) = self
                    .worktree_store
                    .read(cx)
                    .worktree_for_id(project_path.worktree_id, cx)
                else {
                    return Task::ready(Err(anyhow!("no such worktree")));
                };
                let load_buffer = self.local_store.open_buffer(path, worktree, cx);

                entry
                    .insert(
                        cx.spawn(async move |this, cx| {
                            let load_result = load_buffer.await;
                            this.update(cx, |this, _cx| {
                                // Record the fact that the buffer is no longer loading.
                                this.loading_buffers.remove(&project_path);

                                let buffer = load_result.map_err(Arc::new)?;
                                Ok(buffer)
                            })?
                        })
                        .shared(),
                    )
                    .clone()
            }
        };

        cx.background_spawn(async move { task.await.map_err(|error| anyhow!("{error:#}")) })
    }

    pub fn create_buffer(
        &mut self,
        language: Option<Arc<Language>>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Buffer>>> {
        self.local_store.create_buffer(language, cx)
    }

    pub fn save_buffer(
        &mut self,
        buffer: Entity<Buffer>,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        self.local_store.save_buffer(buffer, cx)
    }

    pub fn save_buffer_as(
        &mut self,
        buffer: Entity<Buffer>,
        path: ProjectPath,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let old_file = buffer.read(cx).file().cloned();
        let task = self.local_store.save_buffer_as(buffer.clone(), path, cx);
        cx.spawn(async move |this, cx| {
            task.await?;
            this.update(cx, |this, cx| {
                old_file.clone().and_then(|file| {
                    this.path_to_buffer_id.remove(&ProjectPath {
                        worktree_id: file.worktree_id(cx),
                        path: file.path().clone(),
                    })
                });

                cx.emit(BufferStoreEvent::BufferChangedFilePath { buffer, old_file });
            })
        })
    }

    fn add_buffer(&mut self, buffer_entity: Entity<Buffer>, cx: &mut Context<Self>) -> Result<()> {
        let buffer = buffer_entity.read(cx);
        let remote_id = buffer.remote_id();
        let path = File::from_dyn(buffer.file()).map(|file| ProjectPath {
            path: file.path.clone(),
            worktree_id: file.worktree_id(cx),
        });
        let open_buffer = buffer_entity.downgrade();

        let handle = cx.entity().downgrade();
        buffer_entity.update(cx, move |_, cx| {
            cx.on_release(move |buffer, cx| {
                handle
                    .update(cx, |_, cx| {
                        cx.emit(BufferStoreEvent::BufferDropped(buffer.remote_id()))
                    })
                    .ok();
            })
            .detach()
        });
        match self.opened_buffers.entry(remote_id) {
            hash_map::Entry::Vacant(entry) => {
                entry.insert(open_buffer);
            }
            hash_map::Entry::Occupied(mut entry) => {
                if entry.get().upgrade().is_some() {
                    debug_panic!("buffer {remote_id} was already registered");
                    anyhow::bail!("buffer {remote_id} was already registered");
                }
                entry.insert(open_buffer);
            }
        }

        if let Some(path) = path {
            self.path_to_buffer_id.insert(path, remote_id);
        }

        cx.subscribe(&buffer_entity, Self::on_buffer_event).detach();
        cx.emit(BufferStoreEvent::BufferAdded(buffer_entity));
        Ok(())
    }

    pub fn buffers(&self) -> impl '_ + Iterator<Item = Entity<Buffer>> {
        self.opened_buffers
            .values()
            .filter_map(|buffer| buffer.upgrade())
    }

    pub fn loading_buffers(
        &self,
    ) -> impl Iterator<Item = (&ProjectPath, impl Future<Output = Result<Entity<Buffer>>>)> {
        self.loading_buffers.iter().map(|(path, task)| {
            let task = task.clone();
            (path, async move {
                task.await.map_err(|error| anyhow!("{error:#}"))
            })
        })
    }

    pub fn buffer_id_for_project_path(&self, project_path: &ProjectPath) -> Option<&BufferId> {
        self.path_to_buffer_id.get(project_path)
    }

    pub fn get_by_path(&self, path: &ProjectPath) -> Option<Entity<Buffer>> {
        self.path_to_buffer_id
            .get(path)
            .and_then(|buffer_id| self.get(*buffer_id))
    }

    pub fn get(&self, buffer_id: BufferId) -> Option<Entity<Buffer>> {
        self.opened_buffers.get(&buffer_id)?.upgrade()
    }

    pub fn get_existing(&self, buffer_id: BufferId) -> Result<Entity<Buffer>> {
        self.get(buffer_id)
            .with_context(|| format!("unknown buffer id {buffer_id}"))
    }

    fn buffer_changed_file(&mut self, buffer: Entity<Buffer>, cx: &mut App) -> Option<()> {
        let file = File::from_dyn(buffer.read(cx).file())?;

        let remote_id = buffer.read(cx).remote_id();
        if let Some(entry_id) = file.entry_id {
            match self.local_store.local_buffer_ids_by_entry_id.get(&entry_id) {
                Some(_) => {
                    return None;
                }
                None => {
                    self.local_store
                        .local_buffer_ids_by_entry_id
                        .insert(entry_id, remote_id);
                }
            }
            self.path_to_buffer_id.insert(
                ProjectPath {
                    worktree_id: file.worktree_id(cx),
                    path: file.path.clone(),
                },
                remote_id,
            );
        };

        Some(())
    }

    fn on_buffer_event(
        &mut self,
        buffer: Entity<Buffer>,
        event: &BufferEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            BufferEvent::FileHandleChanged => {
                self.buffer_changed_file(buffer, cx);
            }
            BufferEvent::Reloaded => {}
            BufferEvent::LanguageChanged(_) => {}
            _ => {}
        }
    }

    pub fn reload_buffers(
        &self,
        buffers: HashSet<Entity<Buffer>>,
        push_to_history: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<ProjectTransaction>> {
        if buffers.is_empty() {
            return Task::ready(Ok(ProjectTransaction::default()));
        }
        self.local_store
            .reload_buffers(buffers, push_to_history, cx)
    }

    pub fn create_local_buffer(
        &mut self,
        text: &str,
        language: Option<Arc<Language>>,
        cx: &mut Context<Self>,
    ) -> Entity<Buffer> {
        let buffer = cx.new(|cx| {
            let mut buffer = Buffer::local(text, cx)
                .with_language(language.unwrap_or_else(|| language::PLAIN_TEXT.clone()), cx);
            apply_initial_line_ending(&mut buffer, cx);
            buffer
        });

        self.add_buffer(buffer.clone(), cx).log_err();
        let buffer_id = buffer.read(cx).remote_id();

        if let Some(file) = File::from_dyn(buffer.read(cx).file()) {
            self.path_to_buffer_id.insert(
                ProjectPath {
                    worktree_id: file.worktree_id(cx),
                    path: file.path.clone(),
                },
                buffer_id,
            );
            if let Some(entry_id) = file.entry_id {
                self.local_store
                    .local_buffer_ids_by_entry_id
                    .insert(entry_id, buffer_id);
            }
        }
        buffer
    }
}

fn is_not_found_error(error: &anyhow::Error) -> bool {
    error
        .root_cause()
        .downcast_ref::<io::Error>()
        .is_some_and(|err| err.kind() == io::ErrorKind::NotFound)
}

fn apply_initial_line_ending(buffer: &mut Buffer, cx: &mut Context<Buffer>) {
    // Only applies for empty rope or a single line with no trailing newline.
    if buffer.max_point().row > 0 {
        return;
    }
    let location = buffer.file().map(|file| settings::SettingsLocation {
        worktree_id: file.worktree_id(cx),
        path: file.path().as_ref(),
    });
    let language = buffer.language().map(|l| l.name());
    let settings = AllLanguageSettings::get(location, cx).language(location, language.as_ref(), cx);
    let desired = match settings.line_ending {
        LineEndingSetting::Detect => return,
        LineEndingSetting::PreferLf | LineEndingSetting::EnforceLf => LineEnding::Unix,
        LineEndingSetting::PreferCrlf | LineEndingSetting::EnforceCrlf => LineEnding::Windows,
    };
    if buffer.line_ending() != desired {
        buffer.set_line_ending(desired, cx);
    }
}
