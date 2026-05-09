mod app_menus;
#[cfg(target_os = "macos")]
pub(crate) mod mac_only_instance;
mod open_listener;
#[cfg(target_os = "windows")]
pub(crate) mod windows_only_instance;

use anyhow::Context as _;
pub use app_menus::*;

use crate::release_channel::ReleaseChannel;
use editor::{Editor, MultiBuffer};
use git_ui::commit_view::CommitViewToolbar;
use git_ui::git_panel::GitPanel;
use git_ui::project_diff::{BranchDiffToolbar, ProjectDiffToolbar};
use gpui::{
    App, AppContext as _, Context, Entity, Focusable, PathPromptOptions, PromptLevel, ReadGlobal,
    Task, TitlebarOptions, WeakEntity, Window, WindowHandle, WindowKind, WindowOptions, actions,
    point, px,
};
use language::Capability;
pub use open_listener::*;
use project_panel::ProjectPanel;
use search::project_search::ProjectSearchBar;
use settings::{
    BaseKeymap, DEFAULT_KEYMAP_PATH, KeybindSource, KeymapFile, Settings, SettingsStore,
    update_settings_file,
};

use std::{
    borrow::Cow,
    sync::Arc,
    sync::atomic::{self, AtomicBool},
};
use theme_settings::ThemeSettings;
use ui::prelude::*;
use util::ResultExt;
use uuid::Uuid;

use workspace::Pane;
use workspace::{
    AppState, MultiWorkspace, NewFile, NewWindow, Workspace, WorkspaceSettings, open_new,
};
use workspace::{CloseIntent, CloseProject, CloseWindow};
use zen_actions::{OpenBrowser, OpenZenUrl, Quit};

actions!(
    zen,
    [
        /// Hides the application window.
        Hide,
        /// Hides all other application windows.
        HideOthers,
        /// Minimizes the current window.
        Minimize,
        /// Shows all hidden windows.
        ShowAll,
        /// Toggles fullscreen mode.
        ToggleFullScreen,
        /// Zooms the window.
        Zoom,
    ]
);

pub fn init(cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.on_action(|_: &Hide, cx| cx.hide());
    #[cfg(target_os = "macos")]
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    #[cfg(target_os = "macos")]
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(quit);
}

fn bind_on_window_closed(cx: &mut App) -> Option<gpui::Subscription> {
    #[cfg(target_os = "macos")]
    {
        WorkspaceSettings::get_global(cx)
            .on_last_window_closed
            .is_quit_app()
            .then(|| {
                cx.on_window_closed(|cx, _window_id| {
                    if cx.windows().is_empty() {
                        cx.quit();
                    }
                })
            })
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some(cx.on_window_closed(|cx, _window_id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        }))
    }
}

pub fn build_window_options(display_uuid: Option<Uuid>, cx: &mut App) -> WindowOptions {
    let display = display_uuid.and_then(|uuid| {
        cx.displays()
            .into_iter()
            .find(|display| display.uuid().ok() == Some(uuid))
    });
    let app_id = ReleaseChannel::global(cx).app_id();
    let window_decorations = match std::env::var("ZEN_WINDOW_DECORATIONS") {
        Ok(val) if val == "server" => gpui::WindowDecorations::Server,
        Ok(val) if val == "client" => gpui::WindowDecorations::Client,
        _ => match WorkspaceSettings::get_global(cx).window_decorations {
            settings::WindowDecorations::Server => gpui::WindowDecorations::Server,
            settings::WindowDecorations::Client => gpui::WindowDecorations::Client,
        },
    };

    let use_system_window_tabs = WorkspaceSettings::get_global(cx).use_system_window_tabs;

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    static APP_ICON: std::sync::LazyLock<Option<std::sync::Arc<image::RgbaImage>>> =
        std::sync::LazyLock::new(|| {
            // this shouldn't fail since decode is checked in build.rs
            const BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app_icon.png"));
            util::maybe!({
                let image = image::ImageReader::new(std::io::Cursor::new(BYTES))
                    .with_guessed_format()?
                    .decode()?
                    .into();
                anyhow::Ok(Arc::new(image))
            })
            .log_err()
        });

    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: !cfg!(target_os = "macos"),
            traffic_light_position: if cfg!(target_os = "macos") {
                None
            } else {
                Some(point(px(9.0), px(9.0)))
            },
        }),
        window_bounds: None,
        focus: false,
        show: false,
        kind: WindowKind::Normal,
        is_movable: true,
        display_id: display.map(|display| display.id()),
        window_background: cx.theme().window_background_appearance(),
        app_id: Some(app_id.to_owned()),
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        icon: APP_ICON.as_ref().cloned(),
        window_decorations: Some(window_decorations),
        window_min_size: Some(gpui::Size {
            width: px(360.0),
            height: px(240.0),
        }),
        tabbing_identifier: if use_system_window_tabs {
            Some(String::from("zen"))
        } else {
            None
        },
        ..Default::default()
    }
}

pub fn initialize_workspace(app_state: Arc<AppState>, cx: &mut App) {
    let mut _on_close_subscription = bind_on_window_closed(cx);
    cx.observe_global::<SettingsStore>(move |cx| {
        // A 1.92 regression causes unused-assignment to trigger on this variable.
        _ = _on_close_subscription.is_some();
        _on_close_subscription = bind_on_window_closed(cx);
    })
    .detach();

    cx.observe_new(|_multi_workspace: &mut MultiWorkspace, window, cx| {
        let Some(window) = window else {
            return;
        };

        #[cfg(feature = "track-project-leak")]
        {
            let multi_workspace_handle = cx.weak_entity();
            let workspace_handle = _multi_workspace.workspace().downgrade();
            let project_handle = _multi_workspace.workspace().read(cx).project().downgrade();
            let window_id_2 = window.window_handle().window_id();
            cx.on_window_closed(move |cx, window_id| {
                let multi_workspace_handle = multi_workspace_handle.clone();
                let workspace_handle = workspace_handle.clone();
                let project_handle = project_handle.clone();
                if window_id != window_id_2 {
                    return;
                }
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(1500))
                        .await;

                    multi_workspace_handle.assert_released();
                    workspace_handle.assert_released();
                    project_handle.assert_released();
                })
                .detach();
            })
            .detach();
        }

        let multi_workspace_handle = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            multi_workspace_handle
                .update(cx, |multi_workspace, cx| {
                    // We'll handle closing asynchronously
                    multi_workspace.close_window(&CloseWindow, window, cx);
                    false
                })
                .unwrap_or(true)
        });
    })
    .detach();

    cx.observe_new(move |workspace: &mut Workspace, window, cx| {
        let Some(window) = window else {
            return;
        };

        let workspace_handle = cx.entity();
        let center_pane = workspace.active_pane().clone();
        initialize_pane(workspace, &center_pane, window, cx);

        cx.subscribe_in(&workspace_handle, window, {
            move |workspace, _, event, window, cx| match event {
                workspace::Event::PaneAdded(pane) => {
                    initialize_pane(workspace, pane, window, cx);
                }
                workspace::Event::OpenBundledFile {
                    text,
                    title,
                    language,
                } => open_bundled_file(workspace, text.clone(), title, language, window, cx),
                _ => {}
            }
        })
        .detach();

        #[cfg(not(any(test, target_os = "macos")))]
        initialize_file_watcher(window, cx);

        if let Some(specs) = window.gpu_specs() {
            log::info!("Using GPU: {:?}", specs);
            show_software_emulation_warning_if_needed(specs.clone(), window, cx);
        }

        let search_button = cx.new(|_| search::search_status_button::SearchButton::new());
        let active_file_name = cx.new(|_| workspace::active_file_name::ActiveFileName::new());

        let cursor_position =
            cx.new(|_| go_to_line::cursor_position::CursorPosition::new(workspace));
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.add_left_item(search_button, window, cx);
            status_bar.add_left_item(active_file_name, window, cx);
            status_bar.add_right_item(cursor_position, window, cx);
        });

        let panels_task = initialize_panels(window, cx);
        workspace.set_panels_task(panels_task);
        register_actions(app_state.clone(), workspace, window, cx);

        if !workspace.has_active_modal(window, cx) {
            workspace.focus_handle(cx).focus(window, cx);
        }
    })
    .detach();
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[allow(unused)]
fn initialize_file_watcher(window: &mut Window, cx: &mut Context<Workspace>) {
    if let Err(e) = fs::fs_watcher::global(|_| {}) {
        let message = format!(
            "inotify_init returned {}\n\n\
            This may be due to system-wide limits on inotify instances. \
            For troubleshooting, see the Zen documentation.\n",
            e
        );
        let prompt = window.prompt(
            PromptLevel::Critical,
            "Could not start inotify",
            Some(&message),
            &["Quit"],
            cx,
        );
        cx.spawn(async move |_, cx| {
            if prompt.await == Ok(0) {
                cx.update(|cx| cx.quit());
            }
        })
        .detach()
    }
}

#[cfg(target_os = "windows")]
#[allow(unused)]
fn initialize_file_watcher(window: &mut Window, cx: &mut Context<Workspace>) {
    if let Err(e) = fs::fs_watcher::global(|_| {}) {
        let message = format!(
            "ReadDirectoryChangesW initialization failed: {}\n\n\
            This may occur on network filesystems. \
            For troubleshooting, see the Zen documentation.\n",
            e
        );
        let prompt = window.prompt(
            PromptLevel::Critical,
            "Could not start ReadDirectoryChangesW",
            Some(&message),
            &["Quit"],
            cx,
        );
        cx.spawn(async move |_, cx| {
            if prompt.await == Ok(0) {
                cx.update(|cx| cx.quit());
            }
        })
        .detach()
    }
}

fn show_software_emulation_warning_if_needed(
    specs: gpui::GpuSpecs,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if specs.is_software_emulated && std::env::var("ZEN_ALLOW_EMULATED_GPU").is_err() {
        let graphics_api = if cfg!(target_os = "windows") {
            "DirectX"
        } else {
            "Vulkan"
        };
        let message = format!(
            "Zen uses {} for rendering and requires a compatible GPU.\n\n\
            Currently you are using a software emulated GPU ({}) which\n\
            will result in awful performance.\n\n\
            For troubleshooting, see the Zen documentation.\n\
            Set ZEN_ALLOW_EMULATED_GPU=1 env var to permanently override.\n",
            graphics_api, specs.device_name
        );
        let prompt = window.prompt(
            PromptLevel::Critical,
            "Unsupported GPU",
            Some(&message),
            &["Skip", "Quit"],
            cx,
        );
        cx.spawn(async move |_, cx| {
            if prompt.await == Ok(1) {
                cx.update(|cx| cx.quit());
            }
        })
        .detach()
    }
}

fn initialize_panels(window: &mut Window, cx: &mut Context<Workspace>) -> Task<anyhow::Result<()>> {
    cx.spawn_in(window, async move |workspace_handle, cx| {
        let project_panel = ProjectPanel::load(workspace_handle.clone(), cx.clone());
        let git_panel = GitPanel::load(workspace_handle.clone(), cx.clone());

        async fn add_panel_when_ready(
            panel_task: impl Future<Output = anyhow::Result<Entity<impl workspace::Panel>>> + 'static,
            workspace_handle: WeakEntity<Workspace>,
            mut cx: gpui::AsyncWindowContext,
        ) {
            if let Some(panel) = panel_task.await.context("failed to load panel").log_err()
            {
                workspace_handle
                    .update_in(&mut cx, |workspace, window, cx| {
                        workspace.add_panel(panel, window, cx);
                    })
                    .log_err();
            }
        }

        futures::join!(
            add_panel_when_ready(project_panel, workspace_handle.clone(), cx.clone()),
            add_panel_when_ready(git_panel, workspace_handle.clone(), cx.clone()),
        );

        anyhow::Ok(())
    })
}

fn register_actions(
    app_state: Arc<AppState>,
    workspace: &mut Workspace,
    _: &mut Window,
    _cx: &mut Context<Workspace>,
) {
    workspace
        .register_action(|_, _: &Minimize, window, _| {
            window.minimize_window();
        })
        .register_action(|_, _: &Zoom, window, _| {
            window.zoom_window();
        })
        .register_action(|_, _: &ToggleFullScreen, window, _| {
            window.toggle_fullscreen();
        })
        .register_action(|_, action: &OpenZenUrl, _, cx| {
            OpenListener::global(cx).open(RawOpenRequest {
                urls: vec![action.url.clone()],
                ..Default::default()
            })
        })
        .register_action(|workspace, action: &OpenBrowser, _window, cx| {
            // Parse and validate the URL to ensure it's properly formatted
            match url::Url::parse(&action.url) {
                Ok(parsed_url) => {
                    // Use the parsed URL's string representation which is properly escaped
                    cx.open_url(parsed_url.as_str());
                }
                Err(e) => {
                    workspace.show_error(
                        &anyhow::anyhow!(
                            "Opening this URL in a browser failed because the URL is invalid: {}\n\nError was: {e}",
                            action.url
                        ),
                        cx,
                    );
                }
            }
        })
        .register_action(|workspace, action: &workspace::Open, window, cx| {
            workspace::prompt_for_open_path_and_open(
                workspace,
                workspace.app_state().clone(),
                PathPromptOptions {
                    files: true,
                    directories: true,
                    multiple: true,
                    prompt: None,
                },
                action.create_new_window,
                window,
                cx,
            );
        })
        .register_action(|workspace, _: &workspace::OpenFiles, window, cx| {
            let directories = cx.can_select_mixed_files_and_dirs();
            workspace::prompt_for_open_path_and_open(
                workspace,
                workspace.app_state().clone(),
                PathPromptOptions {
                    files: true,
                    directories,
                    multiple: true,
                    prompt: None,
                },
                true,
                window,
                cx,
            );
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zen_actions::IncreaseUiFontSize, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, cx| {
                        let ui_font_size = ThemeSettings::get_global(cx).ui_font_size(cx) + px(1.0);
                        let _ = settings
                            .theme
                            .ui_font_size
                            .insert(f32::from(theme_settings::clamp_font_size(ui_font_size)).into());
                    });
                } else {
                    theme_settings::adjust_ui_font_size(cx, |size| size + px(1.0));
                }
            }
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zen_actions::DecreaseUiFontSize, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, cx| {
                        let ui_font_size = ThemeSettings::get_global(cx).ui_font_size(cx) - px(1.0);
                        let _ = settings
                            .theme
                            .ui_font_size
                            .insert(f32::from(theme_settings::clamp_font_size(ui_font_size)).into());
                    });
                } else {
                    theme_settings::adjust_ui_font_size(cx, |size| size - px(1.0));
                }
            }
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zen_actions::ResetUiFontSize, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, _| {
                        settings.theme.ui_font_size = None;
                    });
                } else {
                    theme_settings::reset_ui_font_size(cx);
                }
            }
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zen_actions::IncreaseBufferFontSize, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, cx| {
                        let buffer_font_size =
                            ThemeSettings::get_global(cx).buffer_font_size(cx) + px(1.0);
                        let _ = settings
                            .theme
                            .buffer_font_size
                            .insert(f32::from(theme_settings::clamp_font_size(buffer_font_size)).into());
                    });
                } else {
                    theme_settings::increase_buffer_font_size(cx);
                }
            }
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zen_actions::DecreaseBufferFontSize, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, cx| {
                        let buffer_font_size =
                            ThemeSettings::get_global(cx).buffer_font_size(cx) - px(1.0);
                        let _ = settings
                            .theme
                            .buffer_font_size
                            .insert(f32::from(theme_settings::clamp_font_size(buffer_font_size)).into());
                    });
                } else {
                    theme_settings::decrease_buffer_font_size(cx);
                }
            }
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zen_actions::ResetBufferFontSize, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, _| {
                        settings.theme.buffer_font_size = None;
                    });
                } else {
                    theme_settings::reset_buffer_font_size(cx);
                }
            }
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zen_actions::ResetAllZoom, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, _| {
                        settings.theme.ui_font_size = None;
                        settings.theme.buffer_font_size = None;
                    });
                } else {
                    theme_settings::reset_ui_font_size(cx);
                    theme_settings::reset_buffer_font_size(cx);
                }
            }
        })
        .register_action(
            |workspace: &mut Workspace,
             _: &zen_actions::project_panel::ToggleFocus,
             window: &mut Window,
             cx: &mut Context<Workspace>| {
                workspace.toggle_panel_focus::<ProjectPanel>(window, cx);
            },
        )
        .register_action({
            let app_state = app_state.clone();
            move |_, _: &NewWindow, _, cx| {
                open_new(
                    Default::default(),
                    app_state.clone(),
                    cx,
                    |workspace, window, cx| {
                        cx.activate(true);
                        // Create buffer synchronously to avoid flicker
                        let project = workspace.project().clone();
                        let buffer = project.update(cx, |project, cx| {
                            project.create_local_buffer("", None, true, cx)
                        });
                        let editor = cx.new(|cx| {
                            Editor::for_buffer(buffer, Some(project), window, cx)
                        });
                        workspace.add_item_to_active_pane(
                            Box::new(editor),
                            None,
                            true,
                            window,
                            cx,
                        );
                    },
                )
                .detach();
            }
        })
        .register_action({
            let app_state = app_state.clone();
            move |workspace, _: &CloseProject, window, cx| {
                let Some(window_handle) = window.window_handle().downcast::<MultiWorkspace>() else {
                    return;
                };
                let app_state = app_state.clone();
                let old_group_key = workspace.project_group_key(cx);
                cx.spawn_in(window, async move |this, cx| {
                    let should_continue = this
                        .update_in(cx, |workspace, window, cx| {
                            workspace.prepare_to_close(
                                CloseIntent::ReplaceWindow,
                                window,
                                cx,
                            )
                        })?
                        .await?;
                    if should_continue {
                        let task = cx.update(|_window, cx| {
                            open_new(
                                workspace::OpenOptions {
                                    requesting_window: Some(window_handle),
                                    ..Default::default()
                                },
                                app_state,
                                cx,
                                |workspace, window, cx| {
                                    cx.activate(true);
                                    let project = workspace.project().clone();
                                    let buffer = project.update(cx, |project, cx| {
                                        project.create_local_buffer("", None, true, cx)
                                    });
                                    let editor = cx.new(|cx| {
                                        Editor::for_buffer(buffer, Some(project), window, cx)
                                    });
                                    workspace.add_item_to_active_pane(
                                        Box::new(editor),
                                        None,
                                        true,
                                        window,
                                        cx,
                                    );
                                },
                            )
                        })?;
                        task.await?;
                        window_handle.update(cx, |mw, window, cx| {
                            mw.remove_project_group(&old_group_key, window, cx)
                        })?.await.log_err();
                        Ok::<(), anyhow::Error>(())
                    } else {
                        Ok(())
                    }
                })
                .detach_and_log_err(cx);
            }
        })
        .register_action({
            let app_state = app_state.clone();
            move |_, _: &NewFile, _, cx| {
                open_new(
                    Default::default(),
                    app_state.clone(),
                    cx,
                    |workspace, window, cx| {
                        Editor::new_file(workspace, &Default::default(), window, cx)
                    },
                )
                .detach_and_log_err(cx);
            }
        });
}

fn initialize_pane(
    workspace: &Workspace,
    pane: &Entity<Pane>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    pane.update(cx, |pane, cx| {
        pane.toolbar().update(cx, |toolbar, cx| {
            let buffer_search_bar = cx.new(|cx| {
                search::BufferSearchBar::new(
                    Some(workspace.project().read(cx).languages().clone()),
                    window,
                    cx,
                )
            });
            toolbar.add_item(buffer_search_bar.clone(), window, cx);
            let project_search_bar = cx.new(|_| ProjectSearchBar::new());
            toolbar.add_item(project_search_bar, window, cx);
            let project_diff_toolbar = cx.new(|cx| ProjectDiffToolbar::new(workspace, cx));
            toolbar.add_item(project_diff_toolbar, window, cx);
            let branch_diff_toolbar = cx.new(BranchDiffToolbar::new);
            toolbar.add_item(branch_diff_toolbar, window, cx);
            let commit_view_toolbar = cx.new(|_| CommitViewToolbar::new());
            toolbar.add_item(commit_view_toolbar, window, cx);
        })
    });
}

static WAITING_QUIT_CONFIRMATION: AtomicBool = AtomicBool::new(false);
fn quit(_: &Quit, cx: &mut App) {
    if WAITING_QUIT_CONFIRMATION.load(atomic::Ordering::Acquire) {
        return;
    }

    let should_confirm = WorkspaceSettings::get_global(cx).confirm_quit;
    cx.spawn(async move |cx| {
        let mut workspace_windows: Vec<WindowHandle<MultiWorkspace>> = cx.update(|cx| {
            cx.windows()
                .into_iter()
                .filter_map(|window| window.downcast::<MultiWorkspace>())
                .collect::<Vec<_>>()
        });

        // If multiple windows have unsaved changes, and need a save prompt,
        // prompt in the active window before switching to a different window.
        cx.update(|cx| {
            workspace_windows.sort_by_key(|window| window.is_active(cx) == Some(false));
        });

        if should_confirm && let Some(multi_workspace) = workspace_windows.first() {
            let answer = multi_workspace
                .update(cx, |_, window, cx| {
                    window.prompt(
                        PromptLevel::Info,
                        "Are you sure you want to quit?",
                        None,
                        &["Quit", "Cancel"],
                        cx,
                    )
                })
                .log_err();

            if let Some(answer) = answer {
                WAITING_QUIT_CONFIRMATION.store(true, atomic::Ordering::Release);
                let answer = answer.await.ok();
                WAITING_QUIT_CONFIRMATION.store(false, atomic::Ordering::Release);
                if answer != Some(0) {
                    return Ok(());
                }
            }
        }

        // If the user cancels any save prompt, then keep the app open.
        for window in &workspace_windows {
            let window = *window;
            let workspaces = window
                .update(cx, |multi_workspace, _, _cx| {
                    multi_workspace.workspaces().cloned().collect::<Vec<_>>()
                })
                .log_err();

            let Some(workspaces) = workspaces else {
                continue;
            };

            for workspace in workspaces {
                if let Some(should_close) = window
                    .update(cx, |multi_workspace, window, cx| {
                        multi_workspace.activate(workspace.clone(), None, window, cx);
                        window.activate_window();
                        workspace.update(cx, |workspace, cx| {
                            workspace.prepare_to_close(CloseIntent::Quit, window, cx)
                        })
                    })
                    .log_err()
                {
                    if !should_close.await? {
                        return Ok(());
                    }
                }
            }
        }
        // Flush all pending workspace serialization before quitting so that
        // session_id/window_id are up-to-date in the database.
        let mut flush_tasks = Vec::new();
        for window in &workspace_windows {
            window
                .update(cx, |multi_workspace, window, cx| {
                    for workspace in multi_workspace.workspaces() {
                        flush_tasks.push(workspace.update(cx, |workspace, cx| {
                            workspace.flush_serialization(window, cx)
                        }));
                    }
                    flush_tasks.append(&mut multi_workspace.take_pending_removal_tasks());
                    flush_tasks.push(multi_workspace.flush_serialization());
                })
                .log_err();
        }
        futures::future::join_all(flush_tasks).await;

        cx.update(|cx| cx.quit());
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

pub fn load_default_keymap(cx: &mut App) {
    let base_keymap = *BaseKeymap::get_global(cx);
    if base_keymap == BaseKeymap::None {
        return;
    }

    let mut default_key_bindings =
        KeymapFile::load_asset_allow_partial_failure(DEFAULT_KEYMAP_PATH, cx).unwrap();
    for key_binding in &mut default_key_bindings {
        key_binding.set_meta(KeybindSource::Default.meta());
    }
    cx.bind_keys(default_key_bindings);

    if let Some(asset_path) = base_keymap.asset_path() {
        let mut base_key_bindings =
            KeymapFile::load_asset_allow_partial_failure(asset_path, cx).unwrap();
        for key_binding in &mut base_key_bindings {
            key_binding.set_meta(KeybindSource::Base.meta());
        }
        cx.bind_keys(base_key_bindings);
    }
}

fn open_bundled_file(
    workspace: &mut Workspace,
    text: Cow<'static, str>,
    title: &'static str,
    language: &'static str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let existing = workspace.items_of_type::<Editor>(cx).find(|editor| {
        editor.read_with(cx, |editor, cx| {
            editor.read_only(cx)
                && editor.title(cx).as_ref() == title
                && editor
                    .buffer()
                    .read(cx)
                    .as_singleton()
                    .is_some_and(|buffer| buffer.read(cx).file().is_none())
        })
    });
    if let Some(existing) = existing {
        workspace.activate_item(&existing, true, true, window, cx);
        return;
    }

    let language = workspace.app_state().languages.language_for_name(language);
    cx.spawn_in(window, async move |workspace, cx| {
        let language = language.await.log_err();
        workspace
            .update_in(cx, move |workspace, window, cx| {
                let project = workspace.project().clone();
                let buffer = project.update(cx, move |project, cx| {
                    project.create_buffer(language, false, cx)
                });
                cx.spawn_in(window, async move |workspace, cx| {
                    let buffer = buffer.await?;
                    buffer.update(cx, |buffer, cx| {
                        buffer.set_text(text.into_owned(), cx);
                        buffer.set_capability(Capability::ReadOnly, cx);
                    });
                    let buffer =
                        cx.new(|cx| MultiBuffer::singleton(buffer, cx).with_title(title.into()));
                    workspace.update_in(cx, |workspace, window, cx| {
                        workspace.add_item_to_active_pane(
                            Box::new(cx.new(|cx| {
                                let mut editor = Editor::for_multibuffer(
                                    buffer,
                                    Some(project.clone()),
                                    window,
                                    cx,
                                );
                                editor.set_read_only(true);
                                editor.set_should_serialize(false, cx);
                                editor
                            })),
                            None,
                            true,
                            window,
                            cx,
                        )
                    })
                })
            })?
            .await
    })
    .detach_and_log_err(cx);
}
