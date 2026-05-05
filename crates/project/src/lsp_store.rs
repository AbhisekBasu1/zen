use crate::{
    CodeAction, Completion, CompletionDisplayOptions, CompletionResponse, Location, LocationLink,
    Project, ProjectPath, ProjectTransaction, Symbol, buffer_store::BufferStore,
    environment::ProjectEnvironment, lsp_command::LspCommand, manifest_tree::ManifestTree,
    toolchain_store::LocalToolchainStore, worktree_store::WorktreeStore,
};
use anyhow::{Result, anyhow};
use client::proto;
use collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use fs::Fs;
use futures::{FutureExt as _, future::Shared};
use gpui::{
    App, AsyncApp, Context, Entity, EventEmitter, PromptLevel, SharedString, Task, WeakEntity,
};
use language::{
    Buffer, CachedLspAdapter, DiagnosticEntry, Language, LanguageRegistry, Transaction,
};
use lsp::{
    DiagnosticSeverity, LanguageServer, LanguageServerBinary, LanguageServerId, LanguageServerName,
    LanguageServerSelector, MessageActionItem, MessageType, Uri,
};
use rpc::AnyProtoClient;
use serde::Serialize;
use snippet::Snippet;
use std::{
    fmt,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use text::{Anchor, BufferId, PointUtf16};
use worktree::ProjectEntryId;
pub use worktree::WorktreeId;

pub mod clangd_ext {
    use lsp::LanguageServerName;

    pub const CLANGD_SERVER_NAME: LanguageServerName = LanguageServerName::new_static("clangd");
}

pub mod rust_analyzer_ext {
    use gpui::{App, Entity, Task};
    use lsp::LanguageServerName;

    use crate::{Project, ProjectPath};

    pub const RUST_ANALYZER_NAME: LanguageServerName =
        LanguageServerName::new_static("rust-analyzer");
    pub const CARGO_DIAGNOSTICS_SOURCE_NAME: &str = "rustc";

    pub fn cancel_flycheck(
        _: Entity<Project>,
        _: Option<ProjectPath>,
        _: &mut App,
    ) -> Task<anyhow::Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn run_flycheck(
        _: Entity<Project>,
        _: Option<ProjectPath>,
        _: &mut App,
    ) -> Task<anyhow::Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn clear_flycheck(
        _: Entity<Project>,
        _: Option<ProjectPath>,
        _: &mut App,
    ) -> Task<anyhow::Result<()>> {
        Task::ready(Ok(()))
    }
}

pub mod lsp_ext_command {
    use crate::{LocationLink, lsp_command::LspCommand};
    use lsp::LanguageServerId;
    use serde::{Deserialize, Serialize};
    use text::PointUtf16;

    #[derive(Debug)]
    pub enum LspExtExpandMacro {}

    impl lsp::request::Request for LspExtExpandMacro {
        type Params = ExpandMacroParams;
        type Result = Option<ExpandedMacro>;
        const METHOD: &'static str = "rust-analyzer/expandMacro";
    }

    #[derive(Deserialize, Serialize, Debug)]
    #[serde(rename_all = "camelCase")]
    pub struct ExpandMacroParams {
        pub text_document: lsp::TextDocumentIdentifier,
        pub position: lsp::Position,
    }

    #[derive(Default, Deserialize, Serialize, Debug)]
    #[serde(rename_all = "camelCase")]
    pub struct ExpandedMacro {
        pub name: String,
        pub expansion: String,
    }

    impl ExpandedMacro {
        pub fn is_empty(&self) -> bool {
            self.name.is_empty() && self.expansion.is_empty()
        }
    }

    #[derive(Debug)]
    pub struct ExpandMacro {
        pub position: PointUtf16,
    }

    impl LspCommand for ExpandMacro {
        type Response = ExpandedMacro;
        type LspRequest = LspExtExpandMacro;
    }

    #[derive(Debug)]
    pub enum LspOpenDocs {}

    impl lsp::request::Request for LspOpenDocs {
        type Params = OpenDocsParams;
        type Result = Option<DocsUrls>;
        const METHOD: &'static str = "experimental/externalDocs";
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(rename_all = "camelCase")]
    pub struct OpenDocsParams {
        pub text_document: lsp::TextDocumentIdentifier,
        pub position: lsp::Position,
    }

    #[derive(Default, Serialize, Deserialize, Debug)]
    #[serde(rename_all = "camelCase")]
    pub struct DocsUrls {
        pub web: Option<String>,
        pub local: Option<String>,
    }

    impl DocsUrls {
        pub fn is_empty(&self) -> bool {
            self.web.is_none() && self.local.is_none()
        }
    }

    #[derive(Debug)]
    pub struct OpenDocs {
        pub position: PointUtf16,
    }

    impl LspCommand for OpenDocs {
        type Response = DocsUrls;
        type LspRequest = LspOpenDocs;
    }

    #[derive(Debug)]
    pub enum LspSwitchSourceHeader {}

    impl lsp::request::Request for LspSwitchSourceHeader {
        type Params = lsp::TextDocumentIdentifier;
        type Result = Option<String>;
        const METHOD: &'static str = "textDocument/switchSourceHeader";
    }

    #[derive(Debug, Default)]
    pub struct SwitchSourceHeaderResult(pub String);

    #[derive(Debug)]
    pub struct SwitchSourceHeader;

    impl LspCommand for SwitchSourceHeader {
        type Response = SwitchSourceHeaderResult;
        type LspRequest = LspSwitchSourceHeader;
    }

    #[derive(Debug)]
    pub enum LspGoToParentModule {}

    impl lsp::request::Request for LspGoToParentModule {
        type Params = lsp::TextDocumentPositionParams;
        type Result = Option<Vec<lsp::LocationLink>>;
        const METHOD: &'static str = "experimental/parentModule";
    }

    #[derive(Debug)]
    pub struct GoToParentModule {
        pub position: PointUtf16,
    }

    impl LspCommand for GoToParentModule {
        type Response = Vec<LocationLink>;
        type LspRequest = LspGoToParentModule;
    }

    #[derive(Debug)]
    pub struct LspExtCancelFlycheck {}

    #[derive(Debug)]
    pub struct LspExtRunFlycheck {}

    #[derive(Debug)]
    pub struct LspExtClearFlycheck {}

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RunFlycheckParams {
        pub text_document: Option<lsp::TextDocumentIdentifier>,
    }

    #[allow(dead_code)]
    pub type _KeepLanguageServerIdReachable = LanguageServerId;
}

pub mod log_store {
    use collections::HashMap;
    use gpui::{
        App, AppContext as _, Context, Entity, EventEmitter, Global, Subscription, WeakEntity,
    };
    use lsp::{LanguageServer, LanguageServerId, LanguageServerName, MessageType, TraceValue};
    use std::sync::Arc;

    use crate::{LanguageServerLogType, LspStore, Project};

    pub fn init(on_headless_host: bool, cx: &mut App) -> Entity<LogStore> {
        let log_store = cx.new(|cx| LogStore::new(on_headless_host, cx));
        cx.set_global(GlobalLogStore(log_store.clone()));
        log_store
    }

    pub struct GlobalLogStore(pub Entity<LogStore>);

    impl Global for GlobalLogStore {}

    #[derive(Debug)]
    pub enum Event {
        NewServerLogEntry {
            id: LanguageServerId,
            kind: LanguageServerLogType,
            text: String,
        },
    }

    impl EventEmitter<Event> for LogStore {}

    pub trait Message: AsRef<str> {
        type Level: Copy + std::fmt::Debug;
        fn should_include(&self, _: Self::Level) -> bool {
            true
        }
    }

    #[derive(Clone)]
    pub enum LanguageServerKind {
        Local { project: WeakEntity<Project> },
        Remote { project: WeakEntity<Project> },
        LocalSsh { lsp_store: WeakEntity<LspStore> },
        Global,
    }

    impl std::fmt::Debug for LanguageServerKind {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Local { .. } => formatter.write_str("LanguageServerKind::Local"),
                Self::Remote { .. } => formatter.write_str("LanguageServerKind::Remote"),
                Self::LocalSsh { .. } => formatter.write_str("LanguageServerKind::LocalSsh"),
                Self::Global => formatter.write_str("LanguageServerKind::Global"),
            }
        }
    }

    impl LanguageServerKind {
        pub fn project(&self) -> Option<&WeakEntity<Project>> {
            match self {
                Self::Local { project } | Self::Remote { project } => Some(project),
                Self::LocalSsh { .. } | Self::Global => None,
            }
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum LogKind {
        Logs,
        Trace,
        Rpc,
        ServerInfo,
    }

    impl LogKind {
        pub fn from_server_log_type(log_type: &LanguageServerLogType) -> Self {
            match log_type {
                LanguageServerLogType::Log(_) => Self::Logs,
                LanguageServerLogType::Trace { .. } => Self::Trace,
                LanguageServerLogType::Rpc { .. } => Self::Rpc,
            }
        }
    }

    pub struct LanguageServerState {
        pub name: Option<LanguageServerName>,
        pub kind: LanguageServerKind,
        pub logs: Vec<String>,
        pub trace: Vec<String>,
        pub rpc_state: Option<Vec<String>>,
        pub toggled_log_kind: Option<LogKind>,
        pub trace_level: TraceValue,
        #[allow(dead_code)]
        server: Option<Arc<LanguageServer>>,
    }

    pub struct LogStore {
        #[allow(dead_code)]
        on_headless_host: bool,
        #[allow(dead_code)]
        projects: HashMap<WeakEntity<Project>, Vec<Subscription>>,
        pub language_servers: HashMap<LanguageServerId, LanguageServerState>,
    }

    impl LogStore {
        pub fn new(on_headless_host: bool, _: &mut Context<Self>) -> Self {
            Self {
                on_headless_host,
                projects: HashMap::default(),
                language_servers: HashMap::default(),
            }
        }

        pub fn add_project(&mut self, project: &Entity<Project>, _: &mut Context<Self>) {
            self.projects
                .entry(project.downgrade())
                .or_insert_with(Vec::new);
        }

        pub fn add_language_server(
            &mut self,
            kind: LanguageServerKind,
            id: LanguageServerId,
            name: Option<LanguageServerName>,
            _: Option<worktree::WorktreeId>,
            server: Option<Arc<LanguageServer>>,
            _: &mut Context<Self>,
        ) {
            self.language_servers.insert(
                id,
                LanguageServerState {
                    name,
                    kind,
                    logs: Vec::new(),
                    trace: Vec::new(),
                    rpc_state: None,
                    toggled_log_kind: None,
                    trace_level: TraceValue::Off,
                    server,
                },
            );
        }

        pub fn remove_language_server(&mut self, id: LanguageServerId, _: &mut Context<Self>) {
            self.language_servers.remove(&id);
        }

        pub fn get_language_server_state(
            &mut self,
            id: LanguageServerId,
        ) -> Option<&mut LanguageServerState> {
            self.language_servers.get_mut(&id)
        }

        pub fn add_language_server_log(
            &mut self,
            id: LanguageServerId,
            kind: MessageType,
            message: String,
            cx: &mut Context<Self>,
        ) {
            if let Some(state) = self.language_servers.get_mut(&id) {
                state.logs.push(message.clone());
            }
            cx.emit(Event::NewServerLogEntry {
                id,
                kind: LanguageServerLogType::Log(kind),
                text: message,
            });
        }

        pub fn enable_rpc_trace_for_language_server(&mut self, id: LanguageServerId) {
            if let Some(state) = self.language_servers.get_mut(&id) {
                state.rpc_state.get_or_insert_with(Vec::new);
            }
        }

        pub fn disable_rpc_trace_for_language_server(&mut self, id: LanguageServerId) {
            if let Some(state) = self.language_servers.get_mut(&id) {
                state.rpc_state = None;
            }
        }

        pub fn toggle_lsp_logs(
            &mut self,
            id: LanguageServerId,
            enabled: bool,
            toggled_log_kind: LogKind,
        ) {
            if let Some(state) = self.language_servers.get_mut(&id) {
                state.toggled_log_kind = enabled.then_some(toggled_log_kind);
            }
        }
    }
}

pub const SERVER_PROGRESS_THROTTLE_TIMEOUT: Duration = Duration::from_millis(100);
static NEXT_PROMPT_REQUEST_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProgressToken {
    Number(i32),
    String(SharedString),
}

impl fmt::Display for ProgressToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(number) => write!(formatter, "{number}"),
            Self::String(string) => formatter.write_str(string),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatTrigger {
    Save,
    Manual,
}

pub enum LspFormatTarget {
    Buffers,
    Ranges(BTreeMap<BufferId, Vec<Range<Anchor>>>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OpenLspBufferHandle;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SymbolLocation {
    InProject(ProjectPath),
    OutsideProject {
        abs_path: Arc<Path>,
        signature: [u8; 32],
    },
}

#[derive(Debug)]
pub enum LanguageServerToQuery {
    FirstCapable,
    Other(LanguageServerId),
}

pub struct LocalLspStore;

pub struct RemoteLspStore {
    upstream_client: Option<AnyProtoClient>,
    upstream_project_id: u64,
}

enum LspStoreMode {
    Local(LocalLspStore),
    Remote(RemoteLspStore),
}

pub struct LspStore {
    mode: LspStoreMode,
    buffer_store: Entity<BufferStore>,
    worktree_store: Entity<WorktreeStore>,
    environment: Option<Entity<ProjectEnvironment>>,
    last_formatting_failure: Option<String>,
    downstream_client: Option<(AnyProtoClient, u64)>,
    pub languages: Arc<LanguageRegistry>,
    pub language_server_statuses: BTreeMap<LanguageServerId, LanguageServerStatus>,
    pub lsp_server_capabilities: HashMap<LanguageServerId, lsp::ServerCapabilities>,
}

#[derive(Debug)]
pub enum LspStoreEvent {
    LanguageServerAdded(LanguageServerId, LanguageServerName, Option<WorktreeId>),
    LanguageServerRemoved(LanguageServerId),
    LanguageServerUpdate {
        language_server_id: LanguageServerId,
        name: Option<LanguageServerName>,
        message: proto::update_language_server::Variant,
    },
    LanguageServerLog(LanguageServerId, LanguageServerLogType, String),
    LanguageServerPrompt(LanguageServerPromptRequest),
    LanguageDetected {
        buffer: Entity<Buffer>,
        new_language: Option<Arc<Language>>,
    },
    Notification(String),
    RefreshSemanticTokens {
        server_id: LanguageServerId,
        request_id: Option<usize>,
    },
    DiagnosticsUpdated {
        server_id: LanguageServerId,
        paths: Vec<ProjectPath>,
    },
    DiskBasedDiagnosticsStarted {
        language_server_id: LanguageServerId,
    },
    DiskBasedDiagnosticsFinished {
        language_server_id: LanguageServerId,
    },
    SnippetEdit {
        buffer_id: BufferId,
        edits: Vec<(lsp::Range, Snippet)>,
        most_recent_edit: clock::Lamport,
    },
    WorkspaceEditApplied(ProjectTransaction),
}

#[derive(Clone, Debug, Serialize)]
pub struct LanguageServerStatus {
    pub name: LanguageServerName,
    pub server_version: Option<SharedString>,
    pub server_readable_version: Option<SharedString>,
    pub pending_work: BTreeMap<ProgressToken, LanguageServerProgress>,
    pub has_pending_diagnostic_updates: bool,
    pub progress_tokens: HashSet<ProgressToken>,
    pub worktree: Option<WorktreeId>,
    pub binary: Option<LanguageServerBinary>,
    pub configuration: Option<serde_json::Value>,
    pub workspace_folders: BTreeSet<Uri>,
    pub process_id: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct LanguageServerPromptRequest {
    pub id: usize,
    pub level: PromptLevel,
    pub message: String,
    pub actions: Vec<MessageActionItem>,
    pub lsp_name: String,
    response_channel: async_channel::Sender<MessageActionItem>,
}

impl LanguageServerPromptRequest {
    pub fn new(
        level: PromptLevel,
        message: String,
        actions: Vec<MessageActionItem>,
        lsp_name: String,
        response_channel: async_channel::Sender<MessageActionItem>,
    ) -> Self {
        Self {
            id: NEXT_PROMPT_REQUEST_ID.fetch_add(1, Ordering::AcqRel),
            level,
            message,
            actions,
            lsp_name,
            response_channel,
        }
    }

    pub async fn respond(self, index: usize) -> Option<()> {
        let response = self.actions.into_iter().nth(index)?;
        self.response_channel.send(response).await.ok()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn test(
        level: PromptLevel,
        message: String,
        actions: Vec<MessageActionItem>,
        lsp_name: String,
    ) -> Self {
        let (tx, _rx) = async_channel::bounded(1);
        Self::new(level, message, actions, lsp_name, tx)
    }
}

impl PartialEq for LanguageServerPromptRequest {
    fn eq(&self, other: &Self) -> bool {
        self.message == other.message && self.actions == other.actions
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LanguageServerLogType {
    Log(MessageType),
    Trace { verbose_info: Option<String> },
    Rpc { received: bool },
}

#[derive(Clone, Debug, Serialize)]
pub struct LanguageServerProgress {
    pub is_disk_based_diagnostics_progress: bool,
    pub is_cancellable: bool,
    pub title: Option<String>,
    pub message: Option<String>,
    pub percentage: Option<usize>,
    #[serde(skip_serializing)]
    pub last_update_at: Instant,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize)]
pub struct DiagnosticSummary {
    pub error_count: usize,
    pub warning_count: usize,
}

impl DiagnosticSummary {
    pub fn new<'a, T: 'a>(diagnostics: impl IntoIterator<Item = &'a DiagnosticEntry<T>>) -> Self {
        let mut summary = Self::default();
        for entry in diagnostics {
            if entry.diagnostic.is_primary {
                match entry.diagnostic.severity {
                    DiagnosticSeverity::ERROR => summary.error_count += 1,
                    DiagnosticSeverity::WARNING => summary.warning_count += 1,
                    _ => {}
                }
            }
        }
        summary
    }

    pub fn is_empty(&self) -> bool {
        self.error_count == 0 && self.warning_count == 0
    }
}

#[derive(Clone, Debug)]
pub enum CompletionDocumentation {
    Undocumented,
    SingleLine(SharedString),
    MultiLinePlainText(SharedString),
    MultiLineMarkdown(SharedString),
    SingleLineAndMultiLinePlainText {
        single_line: SharedString,
        plain_text: Option<SharedString>,
    },
}

impl CompletionDocumentation {
    #[cfg(any(test, feature = "test-support"))]
    pub fn text(&self) -> SharedString {
        match self {
            Self::Undocumented => "".into(),
            Self::SingleLine(text)
            | Self::MultiLinePlainText(text)
            | Self::MultiLineMarkdown(text) => text.clone(),
            Self::SingleLineAndMultiLinePlainText { single_line, .. } => single_line.clone(),
        }
    }
}

impl From<lsp::Documentation> for CompletionDocumentation {
    fn from(documentation: lsp::Documentation) -> Self {
        match documentation {
            lsp::Documentation::String(text) => {
                if text.lines().count() <= 1 {
                    Self::SingleLine(text.into())
                } else {
                    Self::MultiLinePlainText(text.into())
                }
            }
            lsp::Documentation::MarkupContent(markup) => match markup.kind {
                lsp::MarkupKind::PlainText => Self::MultiLinePlainText(markup.value.into()),
                lsp::MarkupKind::Markdown => Self::MultiLineMarkdown(markup.value.into()),
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RefreshForServer {
    pub server_id: LanguageServerId,
    pub request_id: Option<usize>,
}

pub type SemanticTokensTask =
    Shared<Task<std::result::Result<BufferSemanticTokens, Arc<anyhow::Error>>>>;

#[derive(Debug, Default, Clone)]
pub struct BufferSemanticTokens {
    pub tokens: Option<HashMap<LanguageServerId, Arc<[BufferSemanticToken]>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenType(pub u32);

#[derive(Debug, Clone)]
pub struct BufferSemanticToken {
    pub range: Range<Anchor>,
    pub token_type: TokenType,
    pub token_modifiers: u32,
}

pub struct SemanticTokenStylizer {
    server_id: LanguageServerId,
}

impl SemanticTokenStylizer {
    pub fn server_id(&self) -> LanguageServerId {
        self.server_id
    }

    pub fn token_type_name(&self, _: TokenType) -> Option<&SharedString> {
        None
    }

    pub fn has_modifier(&self, _: u32, _: &str) -> bool {
        false
    }

    pub fn token_modifiers(&self, _: u32) -> Option<String> {
        None
    }

    pub fn rules_for_token(&self, _: TokenType) -> Option<&[settings::SemanticTokenRule]> {
        None
    }
}

impl EventEmitter<LspStoreEvent> for LspStore {}

impl LspStore {
    pub fn init(_: &AnyProtoClient) {}

    pub fn new_local(
        buffer_store: Entity<BufferStore>,
        worktree_store: Entity<WorktreeStore>,
        _: Entity<LocalToolchainStore>,
        environment: Entity<ProjectEnvironment>,
        _: Entity<ManifestTree>,
        languages: Arc<LanguageRegistry>,
        _: Arc<dyn Fs>,
        _: &mut Context<Self>,
    ) -> Self {
        Self {
            mode: LspStoreMode::Local(LocalLspStore),
            buffer_store,
            worktree_store,
            environment: Some(environment),
            last_formatting_failure: None,
            downstream_client: None,
            languages,
            language_server_statuses: BTreeMap::default(),
            lsp_server_capabilities: HashMap::default(),
        }
    }

    pub(super) fn new_remote(
        buffer_store: Entity<BufferStore>,
        worktree_store: Entity<WorktreeStore>,
        languages: Arc<LanguageRegistry>,
        upstream_client: AnyProtoClient,
        project_id: u64,
        _: &mut Context<Self>,
    ) -> Self {
        Self {
            mode: LspStoreMode::Remote(RemoteLspStore {
                upstream_client: Some(upstream_client),
                upstream_project_id: project_id,
            }),
            buffer_store,
            worktree_store,
            environment: None,
            last_formatting_failure: None,
            downstream_client: None,
            languages,
            language_server_statuses: BTreeMap::default(),
            lsp_server_capabilities: HashMap::default(),
        }
    }

    pub fn as_remote(&self) -> Option<&RemoteLspStore> {
        match &self.mode {
            LspStoreMode::Remote(remote) => Some(remote),
            LspStoreMode::Local(_) => None,
        }
    }

    pub fn as_local(&self) -> Option<&LocalLspStore> {
        match &self.mode {
            LspStoreMode::Local(local) => Some(local),
            LspStoreMode::Remote(_) => None,
        }
    }

    pub fn as_local_mut(&mut self) -> Option<&mut LocalLspStore> {
        match &mut self.mode {
            LspStoreMode::Local(local) => Some(local),
            LspStoreMode::Remote(_) => None,
        }
    }

    pub fn shared(&mut self, project_id: u64, client: AnyProtoClient, _: &mut Context<Self>) {
        self.downstream_client = Some((client, project_id));
    }

    pub fn upstream_client(&self) -> Option<(AnyProtoClient, u64)> {
        match &self.mode {
            LspStoreMode::Remote(remote) => remote
                .upstream_client
                .clone()
                .map(|client| (client, remote.upstream_project_id)),
            LspStoreMode::Local(_) => None,
        }
    }

    pub fn disconnected_from_host(&mut self) {
        self.downstream_client = None;
    }

    pub fn disconnected_from_ssh_remote(&mut self) {
        if let LspStoreMode::Remote(remote) = &mut self.mode {
            remote.upstream_client = None;
        }
    }

    pub fn buffer_store(&self) -> Entity<BufferStore> {
        self.buffer_store.clone()
    }

    pub fn set_active_entry(&mut self, _: Option<ProjectEntryId>) {}

    pub fn set_language_server_statuses_from_proto(
        &mut self,
        _: WeakEntity<Project>,
        language_servers: Vec<proto::LanguageServer>,
        server_capabilities: Vec<String>,
        _: &mut Context<Self>,
    ) {
        self.lsp_server_capabilities.clear();
        self.language_server_statuses = language_servers
            .into_iter()
            .zip(server_capabilities)
            .map(|(server, server_capabilities)| {
                let server_id = LanguageServerId::from_proto(server.id);
                if let Ok(capabilities) = serde_json::from_str(&server_capabilities) {
                    self.lsp_server_capabilities.insert(server_id, capabilities);
                }
                let worktree = server.worktree_id.map(WorktreeId::from_proto);
                (
                    server_id,
                    LanguageServerStatus {
                        name: LanguageServerName::from_proto(server.name),
                        server_version: None,
                        server_readable_version: None,
                        pending_work: BTreeMap::default(),
                        has_pending_diagnostic_updates: false,
                        progress_tokens: HashSet::default(),
                        worktree,
                        binary: None,
                        configuration: None,
                        workspace_folders: BTreeSet::default(),
                        process_id: None,
                    },
                )
            })
            .collect();
    }

    pub fn language_server_statuses(
        &self,
    ) -> impl DoubleEndedIterator<Item = (LanguageServerId, &LanguageServerStatus)> {
        self.language_server_statuses
            .iter()
            .map(|(id, status)| (*id, status))
    }

    pub fn supplementary_language_servers(
        &self,
    ) -> impl Iterator<Item = (LanguageServerId, LanguageServerName)> + '_ {
        std::iter::empty()
    }

    pub fn language_servers_running_disk_based_diagnostics(
        &self,
    ) -> impl Iterator<Item = LanguageServerId> + '_ {
        self.language_server_statuses
            .iter()
            .filter_map(|(id, status)| status.has_pending_diagnostic_updates.then_some(*id))
    }

    pub fn diagnostic_summary(&self, _: bool, _: &App) -> DiagnosticSummary {
        DiagnosticSummary::default()
    }

    pub fn diagnostic_summary_for_path(&self, _: &ProjectPath, _: &App) -> DiagnosticSummary {
        DiagnosticSummary::default()
    }

    pub fn diagnostic_summaries(
        &self,
        _: bool,
        _: &App,
    ) -> impl Iterator<Item = (ProjectPath, LanguageServerId, DiagnosticSummary)> + '_ {
        std::iter::empty()
    }

    pub fn last_formatting_failure(&self) -> Option<&str> {
        self.last_formatting_failure.as_deref()
    }

    pub fn reset_last_formatting_failure(&mut self) {
        self.last_formatting_failure = None;
    }

    pub fn supports_range_formatting(&self, _: &Entity<Buffer>, _: &App) -> bool {
        false
    }

    pub fn register_buffer_with_language_servers(
        &mut self,
        _: &Entity<Buffer>,
        _: HashSet<LanguageServerId>,
        _: bool,
        _: &mut App,
    ) -> OpenLspBufferHandle {
        OpenLspBufferHandle
    }

    pub fn set_language_for_buffer(
        &mut self,
        buffer: &Entity<Buffer>,
        new_language: Arc<Language>,
        cx: &mut Context<Self>,
    ) {
        buffer.update(cx, |buffer, cx| {
            buffer.set_language(Some(new_language), cx);
        });
    }

    pub fn restart_language_servers_for_buffers(
        &mut self,
        _: Vec<Entity<Buffer>>,
        _: HashSet<LanguageServerSelector>,
        _: &mut Context<Self>,
    ) {
    }

    pub fn stop_language_servers_for_buffers(
        &mut self,
        _: Vec<Entity<Buffer>>,
        _: HashSet<LanguageServerSelector>,
        _: &mut Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn restart_all_language_servers(&mut self, _: &mut Context<Self>) {}

    pub fn stop_all_language_servers(&mut self, _: &mut Context<Self>) {}

    pub fn cancel_language_server_work_for_buffers(
        &mut self,
        _: impl IntoIterator<Item = Entity<Buffer>>,
        _: &mut Context<Self>,
    ) {
    }

    pub fn cancel_language_server_work(
        &mut self,
        _: LanguageServerId,
        _: Option<ProgressToken>,
        _: &mut Context<Self>,
    ) {
    }

    pub async fn will_rename_entry(
        _: WeakEntity<LspStore>,
        _: WorktreeId,
        _: &Path,
        _: &Path,
        _: bool,
        _: AsyncApp,
    ) -> ProjectTransaction {
        ProjectTransaction::default()
    }

    pub fn did_rename_entry(&self, _: WorktreeId, _: &Path, _: &Path, _: bool) {}

    pub fn format(
        &mut self,
        _: HashSet<Entity<Buffer>>,
        _: LspFormatTarget,
        _: bool,
        _: FormatTrigger,
        _: &mut Context<Self>,
    ) -> Task<Result<ProjectTransaction>> {
        Task::ready(Ok(ProjectTransaction::default()))
    }

    pub fn definitions(
        &mut self,
        _: &Entity<Buffer>,
        _: PointUtf16,
        _: &mut Context<Self>,
    ) -> Task<Result<Option<Vec<LocationLink>>>> {
        Task::ready(Ok(None))
    }

    pub fn declarations(
        &mut self,
        _: &Entity<Buffer>,
        _: PointUtf16,
        _: &mut Context<Self>,
    ) -> Task<Result<Option<Vec<LocationLink>>>> {
        Task::ready(Ok(None))
    }

    pub fn type_definitions(
        &mut self,
        _: &Entity<Buffer>,
        _: PointUtf16,
        _: &mut Context<Self>,
    ) -> Task<Result<Option<Vec<LocationLink>>>> {
        Task::ready(Ok(None))
    }

    pub fn implementations(
        &mut self,
        _: &Entity<Buffer>,
        _: PointUtf16,
        _: &mut Context<Self>,
    ) -> Task<Result<Option<Vec<LocationLink>>>> {
        Task::ready(Ok(None))
    }

    pub fn references(
        &mut self,
        _: &Entity<Buffer>,
        _: PointUtf16,
        _: &mut Context<Self>,
    ) -> Task<Result<Option<Vec<Location>>>> {
        Task::ready(Ok(None))
    }

    pub fn symbols(&mut self, _: &str, _: &mut Context<Self>) -> Task<Result<Vec<Symbol>>> {
        Task::ready(Ok(Vec::new()))
    }

    pub fn open_buffer_for_symbol(
        &mut self,
        _: &Symbol,
        _: &mut Context<Self>,
    ) -> Task<Result<Entity<Buffer>>> {
        Task::ready(Err(anyhow!(
            "project symbols are unavailable in this build"
        )))
    }

    pub fn open_local_buffer_via_lsp(
        &mut self,
        _: Uri,
        _: LanguageServerId,
        _: &mut Context<Self>,
    ) -> Task<Result<Entity<Buffer>>> {
        Task::ready(Err(anyhow!(
            "LSP buffer opening is unavailable in this build"
        )))
    }

    pub fn linked_edits(
        &mut self,
        _: &Entity<Buffer>,
        _: Anchor,
        _: &mut Context<Self>,
    ) -> Task<Result<Vec<Range<Anchor>>>> {
        Task::ready(Ok(Vec::new()))
    }

    pub fn completions(
        &mut self,
        _: &Entity<Buffer>,
        _: PointUtf16,
        _: lsp::CompletionContext,
        _: &mut Context<Self>,
    ) -> Task<Result<Vec<CompletionResponse>>> {
        Task::ready(Ok(vec![CompletionResponse {
            completions: Vec::new(),
            display_options: CompletionDisplayOptions::default(),
            is_incomplete: false,
        }]))
    }

    pub fn resolve_completions(
        &mut self,
        _: Entity<Buffer>,
        _: Vec<usize>,
        _: std::rc::Rc<std::cell::RefCell<Box<[Completion]>>>,
        _: &mut Context<Self>,
    ) -> Task<Result<bool>> {
        Task::ready(Ok(false))
    }

    pub fn apply_additional_edits_for_completion(
        &mut self,
        _: Entity<Buffer>,
        _: std::rc::Rc<std::cell::RefCell<Box<[Completion]>>>,
        _: usize,
        _: bool,
        _: Vec<Range<Anchor>>,
        _: &mut Context<Self>,
    ) -> Task<Result<Option<Transaction>>> {
        Task::ready(Ok(None))
    }

    pub fn code_actions(
        &mut self,
        _: &Entity<Buffer>,
        _: Range<Anchor>,
        _: Option<Vec<lsp::CodeActionKind>>,
        _: &mut Context<Self>,
    ) -> Task<Result<Option<Vec<CodeAction>>>> {
        Task::ready(Ok(None))
    }

    pub fn apply_code_action(
        &mut self,
        _: Entity<Buffer>,
        _: CodeAction,
        _: bool,
        _: &mut Context<Self>,
    ) -> Task<Result<ProjectTransaction>> {
        Task::ready(Ok(ProjectTransaction::default()))
    }

    pub fn apply_code_action_kind(
        &mut self,
        _: HashSet<Entity<Buffer>>,
        _: lsp::CodeActionKind,
        _: bool,
        _: &mut Context<Self>,
    ) -> Task<Result<ProjectTransaction>> {
        Task::ready(Ok(ProjectTransaction::default()))
    }

    pub fn on_type_format<T: text::ToPointUtf16>(
        &mut self,
        _: Entity<Buffer>,
        _: T,
        _: String,
        _: bool,
        _: &mut Context<Self>,
    ) -> Task<Result<Option<Transaction>>> {
        Task::ready(Ok(None))
    }

    pub fn request_lsp<R>(
        &mut self,
        _: Entity<Buffer>,
        _: LanguageServerToQuery,
        _: R,
        _: &mut Context<Self>,
    ) -> Task<Result<R::Response>>
    where
        R: LspCommand,
    {
        Task::ready(Ok(R::Response::default()))
    }

    pub fn semantic_tokens(
        &mut self,
        _: Entity<Buffer>,
        _: Option<RefreshForServer>,
        _: &mut App,
    ) -> SemanticTokensTask {
        Task::ready(Ok(BufferSemanticTokens::default())).shared()
    }

    pub fn get_or_create_token_stylizer(
        &mut self,
        _: LanguageServerId,
        _: Option<&language::LanguageName>,
        _: &mut App,
    ) -> Option<&SemanticTokenStylizer> {
        None
    }

    pub fn pull_diagnostics_for_buffer(
        &mut self,
        _: Entity<Buffer>,
        _: &mut Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn pull_document_diagnostics_for_buffer_edit(
        &mut self,
        _: BufferId,
        _: &mut Context<Self>,
    ) {
    }

    pub fn pull_workspace_diagnostics(&mut self, _: &mut Context<Self>) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn merge_lsp_diagnostics(
        &mut self,
        _: Vec<DocumentDiagnosticsUpdate<'_, lsp::PublishDiagnosticsParams>>,
        _: &mut Context<Self>,
    ) -> Result<()> {
        Ok(())
    }

    pub fn language_server_for_id(&self, _: LanguageServerId) -> Option<Arc<LanguageServer>> {
        None
    }

    pub fn language_server_adapter_for_id(
        &self,
        _: LanguageServerId,
    ) -> Option<Arc<CachedLspAdapter>> {
        None
    }

    pub fn running_language_servers_for_local_buffer<'a>(
        &'a self,
        _: &'a Buffer,
        _: &'a mut App,
    ) -> impl Iterator<Item = Arc<LanguageServer>> + 'a {
        std::iter::empty()
    }

    pub fn environment_for_buffer(
        &self,
        buffer: &Entity<Buffer>,
        cx: &mut Context<Self>,
    ) -> Shared<Task<Option<HashMap<String, String>>>> {
        if let Some(environment) = &self.environment {
            environment.update(cx, |environment, cx| {
                environment.buffer_environment(buffer, &self.worktree_store, cx)
            })
        } else {
            Task::ready(None).shared()
        }
    }
}

pub struct DocumentDiagnosticsUpdate<'a, D> {
    pub language_server_id: LanguageServerId,
    pub path: &'a Path,
    pub diagnostics: D,
}

pub fn glob_literal_prefix(glob: &Path) -> PathBuf {
    glob.components()
        .take_while(|component| match component {
            std::path::Component::Normal(part) => {
                !part.to_string_lossy().contains(['*', '?', '{', '}'])
            }
            _ => true,
        })
        .collect()
}
