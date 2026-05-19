use crate::{
    Autoscroll, Capability, Editor, EditorEvent, MultiBuffer, NavigationData, ReportEditorEvent,
    SelectionEffects,
};
use anyhow::Result;
use collections::HashSet;
use file_icons::FileIcons;
use gpui::{
    AnyElement, App, Context, Entity, EntityId, IntoElement, ParentElement, Pixels, SharedString,
    Styled, Task, Window,
};
use language::{Bias, Buffer, Point};
use multi_buffer::ToPoint as _;
use project::{File, Project, ProjectItem as _, ProjectPath};
use settings::Settings;
use std::{
    any::{Any, TypeId},
    borrow::Cow,
    path::Path,
    sync::Arc,
};
use ui::prelude::*;
use util::{TryFutureExt, paths::PathExt};
use workspace::item::{ItemSettings, TabContentParams};
use workspace::{
    ItemNavHistory, Workspace,
    invalid_item_view::InvalidItemView,
    item::{Item, ItemBufferKind, ItemEvent, ProjectItem, SaveOptions},
};
use workspace::{Pane, item::ProjectItemKind};
use zen_actions::preview::markdown::OpenPreview as OpenMarkdownPreview;

pub const MAX_TAB_TITLE_LEN: usize = 24;

impl Item for Editor {
    type Event = EditorEvent;

    fn act_as_type<'a>(
        &'a self,
        type_id: TypeId,
        self_handle: &'a Entity<Self>,
        cx: &'a App,
    ) -> Option<gpui::AnyEntity> {
        if TypeId::of::<Self>() == type_id {
            Some(self_handle.clone().into())
        } else if TypeId::of::<MultiBuffer>() == type_id {
            Some(self_handle.read(cx).buffer.clone().into())
        } else {
            None
        }
    }

    fn navigate(
        &mut self,
        data: Arc<dyn Any + Send>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(data) = data.downcast_ref::<NavigationData>() {
            let newest_selection = self.selections.newest::<Point>(&self.display_snapshot(cx));
            let buffer = self.buffer.read(cx).read(cx);
            let offset = if buffer.can_resolve(&data.cursor_anchor) {
                data.cursor_anchor.to_point(&buffer)
            } else {
                buffer.clip_point(data.cursor_position, Bias::Left)
            };

            let mut scroll_anchor = data.scroll_anchor;
            if !buffer.can_resolve(&scroll_anchor.anchor) {
                scroll_anchor.anchor = buffer.anchor_before(
                    buffer.clip_point(Point::new(data.scroll_top_row, 0), Bias::Left),
                );
            }

            drop(buffer);

            if newest_selection.head() == offset {
                false
            } else {
                self.set_scroll_anchor(scroll_anchor, window, cx);
                self.change_selections(
                    SelectionEffects::default().nav_history(false),
                    window,
                    cx,
                    |s| s.select_ranges([offset..offset]),
                );
                true
            }
        } else {
            false
        }
    }

    fn tab_tooltip_text(&self, cx: &App) -> Option<SharedString> {
        let multi_buffer = self.buffer().read(cx);
        if let Some(file) = multi_buffer
            .as_singleton()
            .and_then(|buffer| buffer.read(cx).file())
            .and_then(|file| File::from_dyn(Some(file)))
        {
            Some(
                file.worktree
                    .read(cx)
                    .absolutize(&file.path)
                    .compact()
                    .to_string_lossy()
                    .into_owned()
                    .into(),
            )
        } else {
            let title = multi_buffer.title(cx);
            (!title.is_empty()).then(|| title.to_string().into())
        }
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        None
    }

    fn tab_content_text(&self, detail: usize, cx: &App) -> SharedString {
        if let Some(path) = path_for_buffer(&self.buffer, detail, true, cx) {
            path.to_string().into()
        } else {
            // Use the same logic as the displayed title for consistency
            self.buffer.read(cx).title(cx).to_string().into()
        }
    }

    fn suggested_filename(&self, cx: &App) -> SharedString {
        self.buffer.read(cx).title(cx).to_string().into()
    }

    fn tab_icon(&self, _: &Window, cx: &App) -> Option<Icon> {
        ItemSettings::get_global(cx)
            .file_icons
            .then(|| {
                path_for_buffer(&self.buffer, 0, true, cx)
                    .and_then(|path| FileIcons::get_icon(Path::new(&*path), cx))
            })
            .flatten()
            .map(Icon::from_path)
    }

    fn tab_content(&self, params: TabContentParams, _: &Window, cx: &App) -> AnyElement {
        let label_color = self
            .buffer()
            .read(cx)
            .as_singleton()
            .and_then(|buffer| {
                let buffer = buffer.read(cx);
                let path = buffer.project_path(cx)?;
                let project = self.project()?.read(cx);
                let entry = project.entry_for_path(&path, cx)?;
                entry.is_ignored.then_some(Color::Ignored)
            })
            .unwrap_or_else(|| entry_label_color(params.selected));

        let description = params.detail.and_then(|detail| {
            let path = path_for_buffer(&self.buffer, detail, false, cx)?;
            let description = path.trim();

            if description.is_empty() {
                return None;
            }

            Some(util::truncate_and_trailoff(description, MAX_TAB_TITLE_LEN))
        });

        // Whether the file was saved in the past but is now deleted.
        let was_deleted: bool = self
            .buffer()
            .read(cx)
            .as_singleton()
            .and_then(|buffer| buffer.read(cx).file())
            .is_some_and(|file| file.disk_state().is_deleted());

        h_flex()
            .gap_2()
            .child(
                Label::new(util::truncate_and_trailoff(
                    &self.title(cx),
                    MAX_TAB_TITLE_LEN,
                ))
                .color(label_color)
                .when(params.preview, |this| this.italic())
                .when(was_deleted, |this| this.strikethrough()),
            )
            .when_some(description, |this, description| {
                this.child(
                    Label::new(description)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
            .into_any_element()
    }

    fn for_each_project_item(
        &self,
        cx: &App,
        f: &mut dyn FnMut(EntityId, &dyn project::ProjectItem),
    ) {
        self.buffer
            .read(cx)
            .for_each_buffer(&mut |buffer| f(buffer.entity_id(), buffer.read(cx)));
    }

    fn buffer_kind(&self, cx: &App) -> ItemBufferKind {
        match self.buffer.read(cx).is_singleton() {
            true => ItemBufferKind::Singleton,
            false => ItemBufferKind::Multibuffer,
        }
    }

    fn can_save_as(&self, cx: &App) -> bool {
        self.buffer.read(cx).is_singleton()
    }

    fn can_split(&self) -> bool {
        true
    }

    fn clone_on_split(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Option<Entity<Editor>>>
    where
        Self: Sized,
    {
        Task::ready(Some(cx.new(|cx| self.clone(window, cx))))
    }

    fn set_nav_history(
        &mut self,
        history: ItemNavHistory,
        _window: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.nav_history = Some(history);
    }

    fn on_removed(&self, cx: &mut Context<Self>) {
        self.persist_authorship_now(cx);
        self.report_editor_event(ReportEditorEvent::Closed, None, cx);
    }

    fn deactivated(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let selection = self.selections.newest_anchor();
        self.push_to_nav_history(selection.head(), None, true, false, cx);
    }

    fn workspace_deactivated(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.hide_hovered_link(cx);
    }

    fn is_dirty(&self, cx: &App) -> bool {
        self.buffer().read(cx).read(cx).is_dirty()
    }

    fn capability(&self, cx: &App) -> Capability {
        self.capability(cx)
    }

    // Note: this mirrors the logic in `Editor::toggle_read_only`, but is reachable
    // without relying on focus-based action dispatch.
    fn toggle_read_only(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(buffer) = self.buffer.read(cx).as_singleton() {
            buffer.update(cx, |buffer, cx| {
                buffer.set_capability(
                    match buffer.capability() {
                        Capability::ReadWrite => Capability::Read,
                        Capability::Read => Capability::ReadWrite,
                        Capability::ReadOnly => Capability::ReadOnly,
                    },
                    cx,
                );
            });
        }
        cx.notify();
        window.refresh();
    }

    fn has_deleted_file(&self, cx: &App) -> bool {
        self.buffer().read(cx).read(cx).has_deleted_file()
    }

    fn has_conflict(&self, cx: &App) -> bool {
        self.buffer().read(cx).read(cx).has_conflict()
    }

    fn can_save(&self, cx: &App) -> bool {
        let buffer = &self.buffer().read(cx);
        if let Some(buffer) = buffer.as_singleton() {
            buffer.read(cx).project_path(cx).is_some()
        } else {
            true
        }
    }

    fn save(
        &mut self,
        options: SaveOptions,
        project: Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        // Add meta data tracking # of auto saves
        if options.autosave {
            self.report_editor_event(ReportEditorEvent::Saved, None, cx);
        } else {
            self.report_editor_event(ReportEditorEvent::Saved, None, cx);
        }

        let buffers = self.buffer().clone().read(cx).all_buffers();
        let buffers = buffers
            .into_iter()
            .map(|handle| handle.read(cx).base_buffer().unwrap_or(handle.clone()))
            .collect::<HashSet<_>>();

        let buffers_to_save = if self.buffer.read(cx).is_singleton() && !options.autosave {
            buffers
        } else {
            buffers
                .into_iter()
                .filter(|buffer| buffer.read(cx).is_dirty())
                .collect()
        };

        cx.spawn_in(window, async move |_, cx| {
            if !buffers_to_save.is_empty() {
                project
                    .update(cx, |project, cx| {
                        project.save_buffers(buffers_to_save.clone(), cx)
                    })
                    .await?;
            }

            Ok(())
        })
    }

    fn save_as(
        &mut self,
        project: Entity<Project>,
        path: ProjectPath,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let buffer = self
            .buffer()
            .read(cx)
            .as_singleton()
            .expect("cannot call save_as on an excerpt list");

        let file_extension = path.path.extension().map(|a| a.to_string());
        self.report_editor_event(ReportEditorEvent::Saved, file_extension, cx);

        project.update(cx, |project, cx| project.save_buffer_as(buffer, path, cx))
    }

    fn reload(
        &mut self,
        project: Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let buffer = self.buffer().clone();
        let buffers = self.buffer.read(cx).all_buffers();
        let reload_buffers =
            project.update(cx, |project, cx| project.reload_buffers(buffers, true, cx));
        cx.spawn_in(window, async move |this, cx| {
            let transaction = reload_buffers.log_err().await;
            this.update(cx, |editor, cx| {
                editor.request_autoscroll(Autoscroll::fit(), cx)
            })?;
            buffer.update(cx, |buffer, cx| {
                if let Some(transaction) = transaction
                    && !buffer.is_singleton()
                {
                    buffer.push_transaction(&transaction.0, cx);
                }
            });
            Ok(())
        })
    }

    fn pixel_position_of_cursor(&self, _: &App) -> Option<gpui::Point<Pixels>> {
        self.pixel_position_of_newest_cursor
    }

    fn added_to_workspace(
        &mut self,
        workspace: &mut Workspace,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace = Some(workspace.weak_handle());
        if let Some(workspace_entity) = &workspace.weak_handle().upgrade() {
            cx.subscribe(
                workspace_entity,
                |editor, _, event: &workspace::Event, _cx| {
                    if let workspace::Event::ModalOpened = event {
                        editor.mouse_context_menu.take();
                    }
                },
            )
            .detach();
        }
    }

    fn pane_changed(&mut self, _new_pane_id: EntityId, _cx: &mut Context<Self>) {}

    fn to_item_events(event: &EditorEvent, f: &mut dyn FnMut(ItemEvent)) {
        match event {
            EditorEvent::Saved | EditorEvent::TitleChanged => {
                f(ItemEvent::UpdateTab);
            }

            EditorEvent::DirtyChanged => {
                f(ItemEvent::UpdateTab);
            }

            EditorEvent::BufferEdited => {
                f(ItemEvent::Edit);
            }

            EditorEvent::BufferRangesUpdated { .. } | EditorEvent::BuffersRemoved { .. } => {
                f(ItemEvent::Edit);
            }

            _ => {}
        }
    }

    fn tab_extra_context_menu_actions(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<(SharedString, Box<dyn gpui::Action>)> {
        let buffer = self.buffer().read(cx).as_singleton();
        let is_markdown = buffer.is_some_and(|buffer| {
            let buffer = buffer.read(cx);
            buffer
                .language()
                .is_some_and(|language| language.name().as_ref() == "Markdown")
                || buffer
                    .file()
                    .is_some_and(|file| is_markdown_path(&file.full_path(cx)))
        });

        if is_markdown {
            vec![(
                "Open Markdown Preview".into(),
                Box::new(OpenMarkdownPreview) as Box<dyn gpui::Action>,
            )]
        } else {
            Vec::new()
        }
    }

    fn preserve_preview(&self, cx: &App) -> bool {
        self.buffer.read(cx).preserve_preview(cx)
    }
}

fn is_markdown_path(path: &Path) -> bool {
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str())
        && matches!(
            extension.to_ascii_lowercase().as_str(),
            "md" | "mdx" | "mdwn" | "mdc" | "markdown"
        )
    {
        return true;
    }

    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file_name| {
            matches!(
                file_name,
                "README" | "CHANGELOG" | "CONTRIBUTING" | "LICENSE" | "NEWS" | "NOTICE"
            )
        })
}

impl ProjectItem for Editor {
    type Item = Buffer;

    fn project_item_kind() -> Option<ProjectItemKind> {
        Some(ProjectItemKind("Editor"))
    }

    fn for_project_item(
        project: Entity<Project>,
        _pane: Option<&Pane>,
        buffer: Entity<Buffer>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::for_buffer(buffer, Some(project), window, cx)
    }

    fn for_broken_project_item(
        abs_path: &Path,
        is_local: bool,
        e: &anyhow::Error,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<InvalidItemView> {
        Some(InvalidItemView::new(abs_path, is_local, e, window, cx))
    }
}

pub fn entry_label_color(selected: bool) -> Color {
    if selected {
        Color::Default
    } else {
        Color::Muted
    }
}

fn path_for_buffer<'a>(
    buffer: &Entity<MultiBuffer>,
    height: usize,
    include_filename: bool,
    cx: &'a App,
) -> Option<Cow<'a, str>> {
    let file = buffer.read(cx).as_singleton()?.read(cx).file()?;
    path_for_file(file, height, include_filename, cx)
}

fn path_for_file<'a>(
    file: &'a Arc<dyn language::File>,
    mut height: usize,
    include_filename: bool,
    cx: &'a App,
) -> Option<Cow<'a, str>> {
    if project::File::from_dyn(Some(file)).is_none() {
        return None;
    }

    let file = file.as_ref();
    // Ensure we always render at least the filename.
    height += 1;

    let mut prefix = file.path().as_ref();
    while height > 0 {
        if let Some(parent) = prefix.parent() {
            prefix = parent;
            height -= 1;
        } else {
            break;
        }
    }

    // The full_path method allocates, so avoid calling it if height is zero.
    if height > 0 {
        let mut full_path = file.full_path(cx);
        if !include_filename {
            if !full_path.pop() {
                return None;
            }
        }
        Some(full_path.to_string_lossy().into_owned().into())
    } else {
        let mut path = file.path().strip_prefix(prefix).ok()?;
        if !include_filename {
            path = path.parent()?;
        }
        Some(path.display(file.path_style(cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::App;
    use language::TestFile;
    use std::sync::Arc;
    use util::rel_path::RelPath;

    #[gpui::test]
    fn test_path_for_file(cx: &mut App) {
        let file: Arc<dyn language::File> = Arc::new(TestFile {
            path: RelPath::empty().into(),
            root_name: String::new(),
            local_root: None,
        });
        assert_eq!(path_for_file(&file, 0, false, cx), None);
    }
}
