// Disable command line from opening on release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod release_channel;
mod zen;

use anyhow::{Context as _, Result};
use client::{Client, UserStore};
use collections::HashMap;
use editor::Editor;
use fs::{Fs, RealFs};
use futures::StreamExt;
use git::GitHostingProviderRegistry;
use gpui::{App, AppContext, Application, AsyncApp, QuitMode, Task, UpdateGlobal as _};
use gpui_platform;

use crate::release_channel::{AppCommitSha, AppVersion, ReleaseChannel};
use assets::Assets;
use http_client::BlockedHttpClient;
use language::LanguageRegistry;
use parking_lot::Mutex;
use project::trusted_worktrees;
use settings::{Settings, SettingsStore};
use std::{
    env,
    io::{self, IsTerminal},
    path::Path,
    process,
    sync::{Arc, LazyLock, OnceLock},
    time::Instant,
};
use theme::{ActiveTheme, GlobalTheme};
use util::ResultExt;
use uuid::Uuid;
use workspace::{
    AppState, MultiWorkspace, SerializedWorkspaceLocation, Toast, WorkspaceSettings,
    WorkspaceStore, notifications::NotificationId, restore_multiworkspace,
};
use zen::{
    OpenListener, OpenRequest, RawOpenRequest, app_menus, build_window_options,
    derive_paths_with_position, initialize_workspace, open_paths_with_positions,
};

use crate::zen::OpenRequestKind;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn files_not_created_on_launch(errors: HashMap<io::ErrorKind, Vec<&Path>>) {
    let message = "Zen failed to launch";
    let error_details = errors
        .into_iter()
        .flat_map(|(kind, paths)| {
            #[allow(unused_mut)] // for non-unix platforms
            let mut error_kind_details = match paths.len() {
                0 => return None,
                1 => format!(
                    "{kind} when creating directory {:?}",
                    paths.first().expect("match arm checks for a single entry")
                ),
                _many => format!("{kind} when creating directories {paths:?}"),
            };

            #[cfg(unix)]
            {
                if kind == io::ErrorKind::PermissionDenied {
                    error_kind_details.push_str("\n\nConsider using chown and chmod tools for altering the directories permissions if your user has corresponding rights.\
                        \nFor example, `sudo chown $(whoami):staff ~/.config` and `chmod +uwrx ~/.config`");
                }
            }

            Some(error_kind_details)
        })
        .collect::<Vec<_>>().join("\n\n");

    eprintln!("{message}: {error_details}");
    Application::with_platform(gpui_platform::current_platform(false))
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx| {
            if let Ok(window) = cx.open_window(gpui::WindowOptions::default(), |_, cx| {
                cx.new(|_| gpui::Empty)
            }) {
                window
                    .update(cx, |_, window, cx| {
                        let response = window.prompt(
                            gpui::PromptLevel::Critical,
                            message,
                            Some(&error_details),
                            &["Exit"],
                            cx,
                        );

                        cx.spawn_in(window, async move |_, cx| {
                            response.await?;
                            cx.update(|_, cx| cx.quit())
                        })
                        .detach_and_log_err(cx);
                    })
                    .log_err();
            } else {
                fail_to_open_window(anyhow::anyhow!("{message}: {error_details}"), cx)
            }
        })
}

fn fail_to_open_window_async(e: anyhow::Error, cx: &mut AsyncApp) {
    cx.update(|cx| fail_to_open_window(e, cx));
}

fn fail_to_open_window(e: anyhow::Error, _cx: &mut App) {
    eprintln!(
        "Zen failed to open a window: {e:?}. See the Zen documentation for troubleshooting steps."
    );
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        process::exit(1);
    }

    // Maybe unify this with gpui::platform::linux::platform::ResultExt::notify_err(..)?
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        use ashpd::desktop::notification::{Notification, NotificationProxy, Priority};
        _cx.spawn(async move |_cx| {
            let Ok(proxy) = NotificationProxy::new().await else {
                process::exit(1);
            };

            let notification_id = "app.zen.Oops";
            proxy
                .add_notification(
                    notification_id,
                    Notification::new("Zen failed to launch")
                        .body(Some(
                            format!("{e:?}. See the Zen documentation for troubleshooting steps.")
                                .as_str(),
                        ))
                        .priority(Priority::High)
                        .icon(ashpd::desktop::Icon::with_names(&[
                            "dialog-question-symbolic",
                        ])),
                )
                .await
                .ok();

            process::exit(1);
        })
        .detach();
    }
}
static STARTUP_TIME: OnceLock<Instant> = OnceLock::new();
const FORCE_CLI_MODE_ENV_VAR_NAME: &str = "ZEN_FORCE_CLI_MODE";

fn main() {
    STARTUP_TIME.get_or_init(|| Instant::now());

    #[cfg(unix)]
    util::prevent_root_execution();

    let args = Args::parse();

    #[cfg(not(target_os = "windows"))]
    if args.askpass.is_some() {
        eprintln!("askpass is not available in this stripped build");
        process::exit(1);
    }

    #[cfg(target_os = "windows")]
    if args.record_etw_trace {
        let zen_pid = args
            .etw_zen_pid
            .and_then(|pid| if pid >= 0 { Some(pid as u32) } else { None });
        let Some(output_path) = args.etw_output else {
            eprintln!("--etw-output is required for --record-etw-trace");
            process::exit(1);
        };

        let Some(etw_socket) = args.etw_socket else {
            eprintln!("--etw-socket is required for --record-etw-trace");
            process::exit(1);
        };

        if let Err(error) =
            etw_tracing::record_etw_trace(zen_pid, &output_path, etw_socket.as_str())
        {
            eprintln!("ETW trace recording failed: {error:#}");
            process::exit(1);
        }
        return;
    }

    #[cfg(all(not(debug_assertions), target_os = "windows"))]
    unsafe {
        use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

        if args.foreground {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }

    // `zen --printenv` Outputs environment variables as JSON to stdout
    if args.printenv {
        util::shell_env::print_env();
        return;
    }

    // Set custom data directory.
    if let Some(dir) = &args.user_data_dir {
        paths::set_custom_data_dir(dir);
    }

    let file_errors = init_paths();
    if !file_errors.is_empty() {
        files_not_created_on_launch(file_errors);
        return;
    }

    zlog::init();

    if stdout_is_a_pty() {
        zlog::init_output_stdout();
    } else {
        let result = zlog::init_output_file(paths::log_file(), Some(paths::old_log_file()));
        if let Err(err) = result {
            eprintln!("Could not open log file: {}... Defaulting to stdout", err);
            zlog::init_output_stdout();
        };
    }
    ztracing::init();

    let version = option_env!("ZEN_BUILD_ID");
    let app_commit_sha =
        option_env!("ZEN_COMMIT_SHA").map(|commit_sha| AppCommitSha::new(commit_sha.to_string()));
    let app_version = AppVersion::load(env!("CARGO_PKG_VERSION"), version, app_commit_sha.clone());

    log::info!(
        "========== starting Zen version {}, sha {} ==========",
        app_version,
        app_commit_sha
            .as_ref()
            .map(|sha| sha.short())
            .as_deref()
            .unwrap_or("unknown"),
    );

    #[cfg(windows)]
    check_for_conpty_dll();

    let app =
        Application::with_platform(gpui_platform::current_platform(false)).with_assets(Assets);

    let session_id = Uuid::new_v4().to_string();

    let (open_listener, mut open_rx) = OpenListener::new();

    let failed_single_instance_check = if zen_stateless()
        || *crate::release_channel::RELEASE_CHANNEL == ReleaseChannel::Dev
    {
        false
    } else {
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            crate::zen::listen_for_cli_connections(open_listener.clone()).is_err()
        }

        #[cfg(target_os = "windows")]
        {
            !crate::zen::windows_only_instance::handle_single_instance(open_listener.clone(), &args)
        }

        #[cfg(target_os = "macos")]
        {
            use zen::mac_only_instance::*;
            ensure_only_instance() != IsOnlyInstance::Yes
        }
    };
    if failed_single_instance_check {
        println!("Zen is already running");
        return;
    }

    let git_hosting_provider_registry = Arc::new(GitHostingProviderRegistry::new());
    let git_binary_path =
        if cfg!(target_os = "macos") && option_env!("ZEN_BUNDLE").as_deref() == Some("true") {
            app.path_for_auxiliary_executable("git")
                .context("could not find git binary path")
                .log_err()
        } else {
            None
        };
    if let Some(git_binary_path) = &git_binary_path {
        log::info!("Using git binary path: {:?}", git_binary_path);
    }

    let fs = Arc::new(RealFs::new(git_binary_path, app.background_executor()));
    app.on_open_urls({
        let open_listener = open_listener.clone();
        move |urls| {
            open_listener.open(RawOpenRequest {
                urls,
                diff_paths: Vec::new(),
                ..Default::default()
            })
        }
    });
    app.on_reopen(move |cx| {
        if let Some(app_state) = AppState::try_global(cx) {
            cx.spawn({
                async move |cx| {
                    if let Err(e) = restore_or_create_workspace(app_state, cx).await {
                        fail_to_open_window_async(e, cx)
                    }
                }
            })
            .detach();
        }
    });

    app.run(move |cx| {
        trusted_worktrees::init(HashMap::default(), cx);
        menu::init();
        zen_actions::init();

        crate::release_channel::init(app_version, cx);
        settings::init(cx);
        zen::load_default_keymap(cx);

        cx.set_http_client(Arc::new(BlockedHttpClient::new()));

        <dyn Fs>::set_global(fs.clone(), cx);

        GitHostingProviderRegistry::set_global(git_hosting_provider_registry, cx);

        OpenListener::set_global(cx, open_listener.clone());

        let client = Client::production(cx);
        let languages = LanguageRegistry::new(cx.background_executor().clone());
        let languages = Arc::new(languages);
        ui::on_new_scrollbars::<SettingsStore>(cx);

        languages::init(languages.clone(), cx);
        markdown_preview::init(cx);
        let user_store = cx.new(|cx| UserStore::new(client.clone(), cx));
        let workspace_store = cx.new(|_| WorkspaceStore::new());

        zen::init(cx);
        project::Project::init(&client, cx);
        let app_state = Arc::new(AppState {
            languages,
            client: client.clone(),
            user_store,
            fs: fs.clone(),
            build_window_options,
            workspace_store,
            session_id: Arc::from(session_id),
        });
        AppState::set_global(app_state.clone(), cx);

        theme_settings::init(theme::LoadThemes::JustBase, cx);
        load_embedded_fonts(cx);

        editor::init(cx);

        workspace::init(app_state.clone(), cx);

        go_to_line::init(cx);
        file_finder::init(cx);
        project_panel::init(cx);
        search::init(cx);
        cx.set_global(workspace::PaneSearchBarCallbacks {
            setup_search_bar: |languages, toolbar, window, cx| {
                let search_bar = cx.new(|cx| search::BufferSearchBar::new(languages, window, cx));
                toolbar.update(cx, |toolbar, cx| {
                    toolbar.add_item(search_bar, window, cx);
                });
            },
            wrap_div_with_search_actions: search::buffer_search::register_pane_search_actions,
        });
        git_ui::init(cx);
        git_graph::init(cx);
        #[cfg(target_os = "windows")]
        etw_tracing::init(cx);

        cx.observe_global::<SettingsStore>({
            move |cx| {
                for &mut window in cx.windows().iter_mut() {
                    let background_appearance = cx.theme().window_background_appearance();
                    window
                        .update(cx, |_, window, _| {
                            window.set_background_appearance(background_appearance)
                        })
                        .ok();
                }

                cx.set_text_rendering_mode(
                    match WorkspaceSettings::get_global(cx).text_rendering_mode {
                        settings::TextRenderingMode::PlatformDefault => {
                            gpui::TextRenderingMode::PlatformDefault
                        }
                        settings::TextRenderingMode::Subpixel => gpui::TextRenderingMode::Subpixel,
                        settings::TextRenderingMode::Grayscale => {
                            gpui::TextRenderingMode::Grayscale
                        }
                    },
                );
            }
        })
        .detach();
        app_state.languages.set_theme(cx.theme().clone());
        cx.observe_global::<GlobalTheme>({
            let languages = app_state.languages.clone();
            move |cx| {
                languages.set_theme(cx.theme().clone());
            }
        })
        .detach();
        let menus = app_menus(cx);
        cx.set_menus(menus);
        initialize_workspace(app_state.clone(), cx);

        cx.activate(true);

        let urls: Vec<_> = args
            .paths_or_urls
            .iter()
            .map(|arg| parse_url_arg(arg, cx))
            .collect();

        // Check if any diff paths are directories to determine diff_all mode
        let diff_all_mode = args
            .diff
            .chunks(2)
            .any(|pair| Path::new(&pair[0]).is_dir() || Path::new(&pair[1]).is_dir());

        let diff_paths: Vec<[String; 2]> = args
            .diff
            .chunks(2)
            .map(|chunk| [chunk[0].clone(), chunk[1].clone()])
            .collect();

        if !urls.is_empty() || !diff_paths.is_empty() {
            open_listener.open(RawOpenRequest {
                urls,
                diff_paths,
                diff_all: diff_all_mode,
            })
        }

        let restore_task = match open_rx
            .try_recv()
            .ok()
            .and_then(|request| OpenRequest::parse(request, cx).log_err())
        {
            Some(request) => {
                handle_open_request(request, app_state.clone(), cx);
                Task::ready(())
            }
            None => cx.spawn({
                let app_state = app_state.clone();
                async move |cx| {
                    if let Err(e) = restore_or_create_workspace(app_state, cx).await {
                        fail_to_open_window_async(e, cx)
                    }
                }
            }),
        };

        restore_task.detach();

        let app_state = app_state.clone();

        cx.spawn(async move |cx| {
            while let Some(urls) = open_rx.next().await {
                cx.update(|cx| {
                    if let Some(request) = OpenRequest::parse(urls, cx).log_err() {
                        handle_open_request(request, app_state.clone(), cx);
                    }
                });
            }
        })
        .detach();
    });
}

fn handle_open_request(request: OpenRequest, app_state: Arc<AppState>, cx: &mut App) {
    if let Some(kind) = request.kind {
        match kind {
            OpenRequestKind::Extension { extension_id } => {
                log::info!("ignoring extension URL for disabled extension UI: {extension_id}");
            }
            OpenRequestKind::DockMenuAction { index } => {
                cx.perform_dock_menu_action(index);
            }
            OpenRequestKind::BuiltinJsonSchema { schema_path } => {
                log::info!(
                    "ignoring builtin JSON schema URL for disabled schema viewer: {schema_path}"
                );
            }
            OpenRequestKind::Setting { setting_path } => {
                if let Some(setting_path) = setting_path {
                    log::info!(
                        "ignoring settings URL for disabled settings editor: {setting_path}"
                    );
                }
            }
            OpenRequestKind::GitCommit { sha } => {
                cx.spawn(async move |cx| {
                    let paths_with_position =
                        derive_paths_with_position(app_state.fs.as_ref(), request.open_paths).await;
                    let (workspace, _results) = open_paths_with_positions(
                        &paths_with_position,
                        &[],
                        false,
                        app_state,
                        workspace::OpenOptions::default(),
                        cx,
                    )
                    .await?;

                    workspace
                        .update(cx, |multi_workspace, window, cx| {
                            multi_workspace
                                .workspace()
                                .clone()
                                .update(cx, |workspace, cx| {
                                    let Some(repo) =
                                        workspace.project().read(cx).active_repository(cx)
                                    else {
                                        log::error!("no active repository found for commit view");
                                        return Err(anyhow::anyhow!("no active repository found"));
                                    };

                                    git_ui::commit_view::CommitView::open(
                                        sha,
                                        repo.downgrade(),
                                        workspace.weak_handle(),
                                        None,
                                        None,
                                        window,
                                        cx,
                                    );
                                    Ok(())
                                })
                        })
                        .log_err();

                    anyhow::Ok(())
                })
                .detach_and_log_err(cx);
            }
        }

        return;
    }

    let mut task = None;
    if !request.open_paths.is_empty() || !request.diff_paths.is_empty() {
        let app_state = app_state.clone();
        task = Some(cx.spawn(async move |cx| {
            let paths_with_position =
                derive_paths_with_position(app_state.fs.as_ref(), request.open_paths).await;
            let (_window, results) = open_paths_with_positions(
                &paths_with_position,
                &request.diff_paths,
                request.diff_all,
                app_state,
                workspace::OpenOptions::default(),
                cx,
            )
            .await?;
            for result in results.into_iter().flatten() {
                if let Err(err) = result {
                    log::error!("Error opening path: {err}",);
                }
            }
            anyhow::Ok(())
        }));
    }

    if let Some(task) = task {
        cx.spawn(async move |cx| {
            if let Err(err) = task.await {
                fail_to_open_window_async(err, cx);
            }
        })
        .detach();
    }
}

pub(crate) async fn restore_or_create_workspace(
    app_state: Arc<AppState>,
    cx: &mut AsyncApp,
) -> Result<()> {
    if let Some(multi_workspaces) = restorable_workspaces(cx, &app_state).await {
        let mut error_count = 0;
        for multi_workspace in multi_workspaces {
            let result = match &multi_workspace.active_workspace.location {
                SerializedWorkspaceLocation::Local => {
                    restore_multiworkspace(multi_workspace, app_state.clone(), cx)
                        .await
                        .map(|_| ())
                }
                SerializedWorkspaceLocation::Remote(_) => {
                    log::info!("skipping remote workspace restore in stripped build");
                    Ok(())
                }
            };

            if let Err(error) = result {
                log::error!("Failed to restore workspace: {error:#}");
                error_count += 1;
            }
        }

        if error_count > 0 {
            let message = if error_count == 1 {
                "Failed to restore 1 workspace. Check logs for details.".to_string()
            } else {
                format!(
                    "Failed to restore {} workspaces. Check logs for details.",
                    error_count
                )
            };

            // Try to find an active workspace to show the toast
            let toast_shown = cx.update(|cx| {
                if let Some(window) = cx.active_window()
                    && let Some(multi_workspace) = window.downcast::<MultiWorkspace>()
                {
                    multi_workspace
                        .update(cx, |multi_workspace, _, cx| {
                            multi_workspace.workspace().update(cx, |workspace, cx| {
                                workspace.show_toast(
                                    Toast::new(NotificationId::unique::<()>(), message.clone()),
                                    cx,
                                )
                            });
                        })
                        .ok();
                    return true;
                }
                false
            });

            // If we couldn't show a toast (no windows opened successfully),
            // open a fallback empty workspace and show the error there
            if !toast_shown {
                log::error!("All workspace restorations failed. Opening fallback empty workspace.");
                cx.update(|cx| {
                    workspace::open_new(
                        Default::default(),
                        app_state.clone(),
                        cx,
                        |workspace, _window, cx| {
                            workspace.show_toast(
                                Toast::new(NotificationId::unique::<()>(), message),
                                cx,
                            );
                        },
                    )
                })
                .await?;
            }
        }

        // If the user cancelled a failed remote connection at startup,
        // open_remote_project returns Ok but removes the window, so error_count
        // stays 0 and the toast fallback above does not trigger. Without this
        // check, Zen would exit silently.
        if cx.update(|cx| cx.windows().is_empty()) {
            cx.update(|cx| {
                workspace::open_new(
                    Default::default(),
                    app_state.clone(),
                    cx,
                    |workspace, window, cx| {
                        let restore_on_startup =
                            WorkspaceSettings::get_global(cx).restore_on_startup;
                        match restore_on_startup {
                            workspace::RestoreOnStartupBehavior::EmptyTab => {
                                Editor::new_file(workspace, &Default::default(), window, cx);
                            }
                            _ => {
                                // If there was nothing to restore, keep the empty workspace so
                                // the welcome page can show recent projects instead of a blank tab.
                            }
                        }
                    },
                )
            })
            .await?;
        }
    } else {
        cx.update(|cx| {
            workspace::open_new(
                Default::default(),
                app_state,
                cx,
                |workspace, window, cx| {
                    let restore_on_startup = WorkspaceSettings::get_global(cx).restore_on_startup;
                    match restore_on_startup {
                        workspace::RestoreOnStartupBehavior::EmptyTab => {
                            Editor::new_file(workspace, &Default::default(), window, cx);
                        }
                        _ => {
                            // If there was nothing to restore, keep the empty workspace so
                            // the welcome page can show recent projects instead of a blank tab.
                        }
                    }
                },
            )
        })
        .await?;
    }

    Ok(())
}

async fn restorable_workspaces(
    _cx: &mut AsyncApp,
    _app_state: &Arc<AppState>,
) -> Option<Vec<workspace::SerializedMultiWorkspace>> {
    None
}

fn init_paths() -> HashMap<io::ErrorKind, Vec<&'static Path>> {
    [
        paths::config_dir(),
        paths::logs_dir(),
        paths::temp_dir(),
        paths::hang_traces_dir(),
    ]
    .into_iter()
    .fold(HashMap::default(), |mut errors, path| {
        if let Err(e) = std::fs::create_dir_all(path) {
            errors.entry(e.kind()).or_insert_with(Vec::new).push(path);
        }
        errors
    })
}

pub(crate) static FORCE_CLI_MODE: LazyLock<bool> = LazyLock::new(|| {
    let env_var = std::env::var(FORCE_CLI_MODE_ENV_VAR_NAME).ok().is_some();
    unsafe { std::env::remove_var(FORCE_CLI_MODE_ENV_VAR_NAME) };
    env_var
});

fn stdout_is_a_pty() -> bool {
    !*FORCE_CLI_MODE && io::stdout().is_terminal()
}

fn zen_stateless() -> bool {
    std::env::var_os("ZEN_STATELESS").is_some_and(|value| !value.is_empty())
}

#[derive(Debug, Default)]
struct Args {
    paths_or_urls: Vec<String>,
    diff: Vec<String>,
    user_data_dir: Option<String>,
    #[cfg(target_os = "windows")]
    foreground: bool,
    #[cfg(target_os = "windows")]
    dock_action: Option<usize>,
    #[cfg(not(target_os = "windows"))]
    askpass: Option<String>,
    printenv: bool,
    #[cfg(target_os = "windows")]
    record_etw_trace: bool,
    #[cfg(target_os = "windows")]
    etw_zen_pid: Option<i64>,
    #[cfg(target_os = "windows")]
    etw_output: Option<std::path::PathBuf>,
    #[cfg(target_os = "windows")]
    etw_socket: Option<String>,
}

impl Args {
    fn parse() -> Self {
        let mut parsed = Self::default();
        let mut args = env::args().skip(1);

        while let Some(arg) = args.next() {
            if let Some(value) = arg.strip_prefix("--user-data-dir=") {
                parsed.user_data_dir = Some(value.to_string());
                continue;
            }

            #[cfg(not(target_os = "windows"))]
            if let Some(value) = arg.strip_prefix("--askpass=") {
                parsed.askpass = Some(value.to_string());
                continue;
            }

            if arg.strip_prefix("--wsl=").is_some() {
                log::info!("ignoring --wsl in local-only Zen build");
                continue;
            }

            #[cfg(target_os = "windows")]
            if let Some(value) = arg.strip_prefix("--dock-action=") {
                parsed.dock_action = value.parse().ok();
                continue;
            }

            #[cfg(target_os = "windows")]
            if let Some(value) = arg.strip_prefix("--etw-zen-pid=") {
                parsed.etw_zen_pid = value.parse().ok();
                continue;
            }

            #[cfg(target_os = "windows")]
            if let Some(value) = arg.strip_prefix("--etw-output=") {
                parsed.etw_output = Some(value.into());
                continue;
            }

            #[cfg(target_os = "windows")]
            if let Some(value) = arg.strip_prefix("--etw-socket=") {
                parsed.etw_socket = Some(value.to_string());
                continue;
            }

            match arg.as_str() {
                "--diff" => {
                    if let (Some(old_path), Some(new_path)) = (args.next(), args.next()) {
                        parsed.diff.push(old_path);
                        parsed.diff.push(new_path);
                    }
                }
                "--user-data-dir" => {
                    parsed.user_data_dir = args.next();
                }
                "--dev-container" => {
                    log::info!("ignoring --dev-container in local-only Zen build");
                }
                "--dev-server-token" => {
                    args.next();
                }
                "--dump-all-actions" => {
                    log::info!("ignoring --dump-all-actions in local-only Zen build");
                }
                "--printenv" => {
                    parsed.printenv = true;
                }
                #[cfg(not(target_os = "windows"))]
                "--askpass" => {
                    parsed.askpass = args.next();
                }
                "--wsl" => {
                    args.next();
                    log::info!("ignoring --wsl in local-only Zen build");
                }
                #[cfg(target_os = "windows")]
                "--foreground" => {
                    parsed.foreground = true;
                }
                #[cfg(target_os = "windows")]
                "--dock-action" => {
                    parsed.dock_action = args.next().and_then(|value| value.parse().ok());
                }
                #[cfg(target_os = "windows")]
                "--record-etw-trace" => {
                    parsed.record_etw_trace = true;
                }
                #[cfg(target_os = "windows")]
                "--etw-zen-pid" => {
                    parsed.etw_zen_pid = args.next().and_then(|value| value.parse().ok());
                }
                #[cfg(target_os = "windows")]
                "--etw-output" => {
                    parsed.etw_output = args.next().map(Into::into);
                }
                #[cfg(target_os = "windows")]
                "--etw-socket" => {
                    parsed.etw_socket = args.next();
                }
                "--help" | "-h" => {
                    println!("Usage: zen [OPTIONS] [PATH_OR_URL]...");
                    println!("Options: --diff OLD NEW  --user-data-dir DIR");
                    process::exit(0);
                }
                _ => parsed.paths_or_urls.push(arg),
            }
        }

        parsed
    }
}

fn parse_url_arg(arg: &str, _cx: &App) -> String {
    match std::fs::canonicalize(Path::new(&arg)) {
        Ok(path) => format!("file://{}", path.display()),
        Err(_) => {
            if arg.starts_with("file://")
                || arg.starts_with("zen://")
                || arg.starts_with("zen-cli://")
            {
                arg.into()
            } else {
                format!("file://{arg}")
            }
        }
    }
}

fn load_embedded_fonts(cx: &App) {
    let asset_source = cx.asset_source();
    let font_paths = asset_source.list("fonts").unwrap();
    let embedded_fonts = Mutex::new(Vec::new());
    let executor = cx.background_executor();

    cx.foreground_executor().block_on(executor.scoped(|scope| {
        for font_path in &font_paths {
            if !font_path.ends_with(".ttf") {
                continue;
            }

            scope.spawn(async {
                let font_bytes = asset_source.load(font_path).unwrap().unwrap();
                embedded_fonts.lock().push(font_bytes);
            });
        }
    }));

    cx.text_system()
        .add_fonts(embedded_fonts.into_inner())
        .unwrap();
}

#[cfg(target_os = "windows")]
fn check_for_conpty_dll() {
    use windows::{
        Win32::{Foundation::FreeLibrary, System::LibraryLoader::LoadLibraryW},
        core::w,
    };

    if let Ok(hmodule) = unsafe { LoadLibraryW(w!("conpty.dll")) } {
        unsafe {
            FreeLibrary(hmodule)
                .context("Failed to free conpty.dll")
                .log_err();
        }
    } else {
        log::warn!("Failed to load conpty.dll. Terminal will work with reduced functionality.");
    }
}
