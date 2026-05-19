use anyhow::{Context as _, Result, anyhow};
use editor::Editor;
use fs::Fs;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use gpui::{App, AsyncApp, Global, WindowHandle};
use std::path::Path;
use std::sync::Arc;
use util::paths::PathWithPosition;
use util::{ResultExt, percent_decode_uri_component};
use workspace::item::ItemHandle;
use workspace::{AppState, MultiWorkspace, OpenResult};

#[derive(Default, Debug)]
pub struct OpenRequest {
    pub kind: Option<OpenRequestKind>,
    pub open_paths: Vec<String>,
    pub diff_paths: Vec<[String; 2]>,
    pub diff_all: bool,
}

pub enum OpenRequestKind {
    DockMenuAction {
        index: usize,
    },
    BuiltinJsonSchema {
        schema_path: String,
    },
    Setting {
        /// `None` opens settings without navigating to a specific path.
        setting_path: Option<String>,
    },
}

impl std::fmt::Debug for OpenRequestKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DockMenuAction { index } => f
                .debug_struct("DockMenuAction")
                .field("index", index)
                .finish(),
            Self::BuiltinJsonSchema { schema_path } => f
                .debug_struct("BuiltinJsonSchema")
                .field("schema_path", schema_path)
                .finish(),
            Self::Setting { setting_path } => f
                .debug_struct("Setting")
                .field("setting_path", setting_path)
                .finish(),
        }
    }
}

impl OpenRequest {
    pub fn parse(request: RawOpenRequest, _cx: &App) -> Result<Self> {
        let mut this = Self::default();

        this.diff_paths = request.diff_paths;
        this.diff_all = request.diff_all;

        for url in request.urls {
            if let Some(server_name) = url.strip_prefix("zen-cli://") {
                log::info!("ignoring CLI IPC URL in stripped build: {server_name}");
            } else if let Some(action_index) = url.strip_prefix("zen-dock-action://") {
                this.kind = Some(OpenRequestKind::DockMenuAction {
                    index: action_index.parse()?,
                });
            } else if let Some(file) = url.strip_prefix("file://") {
                this.parse_file_path(file)
            } else if let Some(file) = url.strip_prefix("zen://file") {
                this.parse_file_path(file)
            } else if let Some(schema_path) = url.strip_prefix("zen://schemas/") {
                this.kind = Some(OpenRequestKind::BuiltinJsonSchema {
                    schema_path: schema_path.to_string(),
                });
            } else if url == "zen://settings" || url == "zen://settings/" {
                this.kind = Some(OpenRequestKind::Setting { setting_path: None });
            } else if let Some(setting_path) = url.strip_prefix("zen://settings/") {
                this.kind = Some(OpenRequestKind::Setting {
                    setting_path: Some(setting_path.to_string()),
                });
            } else {
                log::error!("unhandled url: {}", url);
            }
        }

        Ok(this)
    }

    fn parse_file_path(&mut self, file: &str) {
        if let Some(decoded) = percent_decode_uri_component(file).log_err() {
            self.open_paths.push(decoded.into_owned())
        }
    }
}

#[derive(Clone)]
pub struct OpenListener(UnboundedSender<RawOpenRequest>);

#[derive(Default)]
pub struct RawOpenRequest {
    pub urls: Vec<String>,
    pub diff_paths: Vec<[String; 2]>,
    pub diff_all: bool,
}

impl Global for OpenListener {}

impl OpenListener {
    pub fn new() -> (Self, UnboundedReceiver<RawOpenRequest>) {
        let (tx, rx) = mpsc::unbounded();
        (OpenListener(tx), rx)
    }

    pub fn open(&self, request: RawOpenRequest) {
        self.0
            .unbounded_send(request)
            .context("no listener for open requests")
            .log_err();
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub fn listen_for_cli_connections(_opener: OpenListener) -> Result<()> {
    Ok(())
}

pub async fn open_paths_with_positions(
    path_positions: &[PathWithPosition],
    diff_paths: &[[String; 2]],
    _diff_all: bool,
    app_state: Arc<AppState>,
    open_options: workspace::OpenOptions,
    cx: &mut AsyncApp,
) -> Result<(
    WindowHandle<MultiWorkspace>,
    Vec<Option<Result<Box<dyn ItemHandle>>>>,
)> {
    let paths = path_positions
        .iter()
        .map(|path_with_position| path_with_position.path.clone())
        .collect::<Vec<_>>();

    let OpenResult {
        window: multi_workspace,
        opened_items: mut items,
        ..
    } = cx
        .update(|cx| workspace::open_paths(&paths, app_state, open_options, cx))
        .await?;

    if !diff_paths.is_empty() {
        log::info!("ignoring diff open request in stripped build");
    }

    for (item, path) in items.iter_mut().zip(&paths) {
        if let Some(Err(error)) = item {
            *error = anyhow!("error opening {path:?}: {error}");
        }
    }

    let items_for_navigation = items
        .iter()
        .map(|item| item.as_ref().and_then(|r| r.as_ref().ok()).cloned())
        .collect::<Vec<_>>();
    navigate_to_positions(&multi_workspace, items_for_navigation, path_positions, cx);

    Ok((multi_workspace, items))
}

fn navigate_to_positions(
    window: &WindowHandle<MultiWorkspace>,
    items: impl IntoIterator<Item = Option<Box<dyn ItemHandle>>>,
    positions: &[PathWithPosition],
    cx: &mut AsyncApp,
) {
    for (item, path) in items.into_iter().zip(positions) {
        let Some(item) = item else {
            continue;
        };
        let Some(row) = path.row else {
            continue;
        };
        if let Some(active_editor) = item.downcast::<Editor>() {
            window
                .update(cx, |_, window, cx| {
                    active_editor.update(cx, |editor, cx| {
                        let row = row.saturating_sub(1);
                        let column = path.column.unwrap_or(0).saturating_sub(1);
                        let Some(buffer) = editor.buffer().read(cx).as_singleton() else {
                            return;
                        };
                        let buffer_snapshot = buffer.read(cx).snapshot();
                        let point = buffer_snapshot.point_from_external_input(row, column);
                        editor.go_to_singleton_buffer_point(point, window, cx);
                    });
                })
                .ok();
        }
    }
}

pub async fn derive_paths_with_position(
    fs: &dyn Fs,
    path_strings: impl IntoIterator<Item = impl AsRef<str>>,
) -> Vec<PathWithPosition> {
    let path_strings: Vec<_> = path_strings.into_iter().collect();
    let mut result = Vec::with_capacity(path_strings.len());
    for path_str in path_strings {
        let original_path = Path::new(path_str.as_ref());
        let mut parsed = PathWithPosition::parse_str(path_str.as_ref());

        // If the unparsed path string actually points to a file, use that file instead of parsing out the line/col number.
        // Note: The colon syntax is also used to open NTFS alternate data streams (e.g., `file.txt:stream`), which would cause issues.
        // However, the colon is not valid in NTFS file names, so we can just skip this logic.
        if !cfg!(windows)
            && parsed.row.is_some()
            && parsed.path != original_path
            && fs.is_file(original_path).await
        {
            parsed = PathWithPosition::from_path(original_path.to_path_buf());
        }

        if let Ok(canonicalized) = fs.canonicalize(&parsed.path).await {
            parsed.path = canonicalized;
        }

        result.push(parsed);
    }
    result
}
