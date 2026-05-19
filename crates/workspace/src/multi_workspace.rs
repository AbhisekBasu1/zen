use anyhow::Result;
use gpui::{
    App, Context, Entity, Focusable, IntoElement, ManagedView, ParentElement, Render, Task, Tiling,
    Window,
};
#[cfg(any(test, feature = "test-support"))]
use project::Project;
use std::path::PathBuf;
use ui::prelude::*;

use crate::{
    CloseIntent, CloseWindow, DockPosition, Event as WorkspaceEvent, Item, ModalView, OpenMode,
    Panel, Workspace, client_side_decorations,
};

pub struct MultiWorkspace {
    active_workspace: Entity<Workspace>,
}

impl MultiWorkspace {
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::subscribe_to_workspace(&workspace, window, cx);
        Self {
            active_workspace: workspace,
        }
    }

    fn subscribe_to_workspace(
        workspace: &Entity<Workspace>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(workspace, window, |this, workspace, event, window, cx| {
            if let WorkspaceEvent::Activate = event {
                this.activate(workspace.clone(), window, cx);
            }
        })
        .detach();
    }

    fn register_workspace(
        &mut self,
        workspace: &Entity<Workspace>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        Self::subscribe_to_workspace(workspace, window, cx);
    }

    pub fn close_window(&mut self, _: &CloseWindow, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = self.workspace().clone();
        cx.spawn_in(window, async move |_this, cx| {
            let should_continue = workspace
                .update_in(cx, |workspace, window, cx| {
                    workspace.prepare_to_close(CloseIntent::CloseWindow, window, cx)
                })?
                .await?;

            if should_continue {
                cx.update(|window, _cx| {
                    window.remove_window();
                })?;
            }

            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
    }

    pub fn workspace(&self) -> &Entity<Workspace> {
        &self.active_workspace
    }

    pub fn workspaces(&self) -> impl Iterator<Item = &Entity<Workspace>> {
        std::iter::once(&self.active_workspace)
    }

    pub fn activate(
        &mut self,
        workspace: Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace() == &workspace {
            self.focus_active_workspace(window, cx);
            return;
        }

        self.register_workspace(&workspace, window, cx);
        self.active_workspace = workspace;
        self.focus_active_workspace(window, cx);
        cx.notify();
    }

    pub fn focus_active_workspace(&self, window: &mut Window, cx: &mut App) {
        let focus_handle = {
            let workspace = self.workspace().read(cx);
            let mut target = None;
            for dock in workspace.all_docks() {
                let dock = dock.read(cx);
                if dock.is_open()
                    && let Some(panel) = dock.active_panel()
                    && panel.is_zoomed(window, cx)
                {
                    target = Some(panel.panel_focus_handle(cx));
                    break;
                }
            }
            target.unwrap_or_else(|| {
                let pane = workspace.active_pane().clone();
                pane.read(cx).focus_handle(cx)
            })
        };
        window.focus(&focus_handle, cx);
    }

    pub fn panel<T: Panel>(&self, cx: &App) -> Option<Entity<T>> {
        self.workspace().read(cx).panel::<T>(cx)
    }

    pub fn active_modal<V: ManagedView + 'static>(&self, cx: &App) -> Option<Entity<V>> {
        self.workspace().read(cx).active_modal::<V>(cx)
    }

    pub fn add_panel<T: Panel>(
        &mut self,
        panel: Entity<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace().update(cx, |workspace, cx| {
            workspace.add_panel(panel, window, cx);
        });
    }

    pub fn focus_panel<T: Panel>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<T>> {
        self.workspace()
            .update(cx, |workspace, cx| workspace.focus_panel::<T>(window, cx))
    }

    pub fn toggle_modal<V: ModalView, B>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        build: B,
    ) where
        B: FnOnce(&mut Window, &mut gpui::Context<V>) -> V,
    {
        self.workspace().update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, build);
        });
    }

    pub fn toggle_dock(
        &mut self,
        dock_side: DockPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace().update(cx, |workspace, cx| {
            workspace.toggle_dock(dock_side, window, cx);
        });
    }

    pub fn active_item_as<I: 'static>(&self, cx: &App) -> Option<Entity<I>> {
        self.workspace().read(cx).active_item_as::<I>(cx)
    }

    pub fn items_of_type<'a, T: Item>(
        &'a self,
        cx: &'a App,
    ) -> impl 'a + Iterator<Item = Entity<T>> {
        self.workspace().read(cx).items_of_type::<T>(cx)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn test_new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let workspace = cx.new(|cx| Workspace::test_new(project, window, cx));
        Self::new(workspace, window, cx)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn create_test_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let app_state = self.workspace().read(cx).app_state().clone();
        let project = Project::local(
            app_state.languages.clone(),
            app_state.fs.clone(),
            project::LocalProjectFlags::default(),
            cx,
        );
        let new_workspace = cx.new(|cx| Workspace::new(project, app_state, window, cx));
        self.activate(new_workspace, window, cx);
        Task::ready(())
    }

    pub fn open_project(
        &mut self,
        paths: Vec<PathBuf>,
        open_mode: OpenMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Workspace>>> {
        let workspace = self.workspace().clone();
        cx.spawn_in(window, async move |_this, cx| {
            let should_continue = workspace
                .update_in(cx, |workspace, window, cx| {
                    workspace.prepare_to_close(CloseIntent::ReplaceWindow, window, cx)
                })?
                .await?;

            if should_continue {
                workspace
                    .update_in(cx, |workspace, window, cx| {
                        workspace.open_workspace_for_paths(open_mode, paths, window, cx)
                    })?
                    .await
            } else {
                Ok(workspace)
            }
        })
    }
}

impl Render for MultiWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui_font = theme_settings::setup_ui_font(window, cx);
        let text_color = cx.theme().colors().text;

        let workspace = self.workspace().clone();
        let workspace_key_context = workspace.update(cx, |workspace, cx| workspace.key_context(cx));
        let root = workspace.update(cx, |workspace, cx| workspace.actions(h_flex(), window, cx));

        client_side_decorations(
            root.key_context(workspace_key_context)
                .relative()
                .size_full()
                .font(ui_font)
                .text_color(text_color)
                .on_action(cx.listener(Self::close_window))
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .size_full()
                        .overflow_hidden()
                        .child(self.workspace().clone()),
                )
                .child(self.workspace().read(cx).modal_layer.clone()),
            window,
            cx,
            Tiling::default(),
        )
    }
}
