pub use lsp_types::request::*;
pub use lsp_types::*;

use anyhow::{Result, anyhow};
use collections::HashMap;
use gpui::{App, AsyncApp, SharedString, Task};
use parking_lot::{Mutex, RwLock};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fmt,
    future::{Future, ready},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
    task::Poll,
    time::Duration,
};
use util::{ConnectionResult, redact};

pub const DEFAULT_LSP_REQUEST_TIMEOUT_SECS: u64 = 120;
pub const DEFAULT_LSP_REQUEST_TIMEOUT: Duration =
    Duration::from_secs(DEFAULT_LSP_REQUEST_TIMEOUT_SECS);

#[derive(Debug, Clone, Copy)]
pub enum IoKind {
    StdOut,
    StdIn,
    StdErr,
}

#[derive(Clone, Serialize)]
pub struct LanguageServerBinary {
    pub path: PathBuf,
    pub arguments: Vec<OsString>,
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone)]
pub struct LanguageServerBinaryOptions {
    pub allow_path_lookup: bool,
    pub allow_binary_download: bool,
    pub pre_release: bool,
}

pub struct LanguageServer {
    server_id: LanguageServerId,
    next_id: AtomicI32,
    name: LanguageServerName,
    version: Option<SharedString>,
    process_name: Arc<str>,
    binary: LanguageServerBinary,
    capabilities: RwLock<ServerCapabilities>,
    configuration: Arc<DidChangeConfigurationParams>,
    code_action_kinds: Option<Vec<CodeActionKind>>,
    workspace_folders: Option<Arc<Mutex<BTreeSet<Uri>>>>,
    root_uri: Option<Uri>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LanguageServerSelector {
    Id(LanguageServerId),
    Name(LanguageServerName),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct LanguageServerId(pub usize);

impl LanguageServerId {
    pub fn from_proto(id: u64) -> Self {
        Self(id as usize)
    }

    pub fn to_proto(self) -> u64 {
        self.0 as u64
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, JsonSchema,
)]
#[serde(transparent)]
pub struct LanguageServerName(pub SharedString);

impl std::fmt::Display for LanguageServerName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, formatter)
    }
}

impl AsRef<str> for LanguageServerName {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl AsRef<OsStr> for LanguageServerName {
    fn as_ref(&self) -> &OsStr {
        self.0.as_ref().as_ref()
    }
}

impl LanguageServerName {
    pub const fn new_static(s: &'static str) -> Self {
        Self(SharedString::new_static(s))
    }

    pub fn from_proto(s: String) -> Self {
        Self(s.into())
    }
}

impl<'a> From<&'a str> for LanguageServerName {
    fn from(str: &'a str) -> LanguageServerName {
        LanguageServerName(str.to_string().into())
    }
}

impl PartialEq<str> for LanguageServerName {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

pub enum Subscription {
    Detached,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Int(i32),
    Str(String),
}

pub trait LspRequestFuture<O>: Future<Output = ConnectionResult<O>> {
    fn id(&self) -> i32;
}

pub struct Request<'a, T>
where
    T: 'static,
{
    _method: &'a str,
    _params: T,
}

struct LspRequest<F> {
    id: i32,
    request: F,
}

impl<F> LspRequest<F> {
    fn new(id: i32, request: F) -> Self {
        Self { id, request }
    }
}

impl<F: Future> Future for LspRequest<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        // SAFETY: This is standard pin projection, we're pinned so our fields must be pinned.
        let inner = unsafe { Pin::new_unchecked(&mut self.get_unchecked_mut().request) };
        inner.poll(cx)
    }
}

impl<F, O> LspRequestFuture<O> for LspRequest<F>
where
    F: Future<Output = ConnectionResult<O>>,
{
    fn id(&self) -> i32 {
        self.id
    }
}

#[derive(Debug, Clone)]
pub struct AdapterServerCapabilities {
    pub server_capabilities: ServerCapabilities,
    pub code_action_kinds: Option<Vec<CodeActionKind>>,
}

pub const SEMANTIC_TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::CLASS,
    SemanticTokenType::ENUM,
    SemanticTokenType::INTERFACE,
    SemanticTokenType::STRUCT,
    SemanticTokenType::TYPE_PARAMETER,
    SemanticTokenType::TYPE,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::DECORATOR,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::METHOD,
    SemanticTokenType::MACRO,
    SemanticTokenType::new("label"),
    SemanticTokenType::COMMENT,
    SemanticTokenType::STRING,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::NUMBER,
    SemanticTokenType::REGEXP,
    SemanticTokenType::OPERATOR,
    SemanticTokenType::MODIFIER,
    SemanticTokenType::EVENT,
    SemanticTokenType::new("lifetime"),
];

pub const SEMANTIC_TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::READONLY,
    SemanticTokenModifier::STATIC,
    SemanticTokenModifier::DEPRECATED,
    SemanticTokenModifier::ABSTRACT,
    SemanticTokenModifier::ASYNC,
    SemanticTokenModifier::MODIFICATION,
    SemanticTokenModifier::DOCUMENTATION,
    SemanticTokenModifier::DEFAULT_LIBRARY,
    SemanticTokenModifier::new("constant"),
];

impl LanguageServer {
    pub fn new(
        _stderr_capture: Arc<Mutex<Option<String>>>,
        server_id: LanguageServerId,
        server_name: LanguageServerName,
        binary: LanguageServerBinary,
        root_path: &Path,
        code_action_kinds: Option<Vec<CodeActionKind>>,
        workspace_folders: Option<Arc<Mutex<BTreeSet<Uri>>>>,
        _cx: &mut AsyncApp,
    ) -> Result<Self> {
        let working_dir = if root_path.is_dir() {
            root_path
        } else {
            root_path.parent().unwrap_or_else(|| Path::new("/"))
        };
        let root_uri = Uri::from_file_path(working_dir)
            .map_err(|()| anyhow!("{working_dir:?} is not a valid URI"))?;
        let process_name = binary
            .path
            .file_name()
            .map(|name| Arc::from(name.to_string_lossy()))
            .unwrap_or_default();

        Ok(Self {
            server_id,
            next_id: AtomicI32::new(0),
            name: server_name,
            version: None,
            process_name,
            binary,
            capabilities: Default::default(),
            configuration: Arc::new(DidChangeConfigurationParams {
                settings: serde_json::Value::Null,
            }),
            code_action_kinds,
            workspace_folders,
            root_uri: Some(root_uri),
        })
    }

    pub fn full_capabilities() -> ServerCapabilities {
        ServerCapabilities::default()
    }

    pub fn code_action_kinds(&self) -> Option<Vec<CodeActionKind>> {
        self.code_action_kinds.clone()
    }

    pub fn default_initialize_params(
        &self,
        _pull_diagnostics: bool,
        _augments_syntax_tokens: bool,
        _cx: &App,
    ) -> InitializeParams {
        let workspace_folders = self.workspace_folders();
        let workspace_folders = workspace_folders
            .into_iter()
            .map(|uri| WorkspaceFolder {
                name: String::new(),
                uri,
            })
            .collect::<Vec<_>>();

        #[allow(deprecated)]
        InitializeParams {
            process_id: Some(std::process::id()),
            root_path: self.root_uri.as_ref().and_then(|root_uri| {
                root_uri
                    .to_file_path()
                    .ok()
                    .map(|path| path.to_string_lossy().into_owned())
            }),
            root_uri: self.root_uri.clone(),
            capabilities: ClientCapabilities::default(),
            workspace_folders: Some(workspace_folders),
            ..InitializeParams::default()
        }
    }

    pub fn initialize(
        mut self,
        _params: InitializeParams,
        configuration: Arc<DidChangeConfigurationParams>,
        _timeout: Duration,
        _cx: &App,
    ) -> Task<Result<Arc<Self>>> {
        self.configuration = configuration;
        Task::ready(Ok(Arc::new(self)))
    }

    pub fn shutdown(&self) -> Option<impl 'static + Send + Future<Output = Option<()>> + use<>> {
        None::<std::future::Ready<Option<()>>>
    }

    #[must_use]
    pub fn on_notification<T, F>(&self, _f: F) -> Subscription
    where
        T: notification::Notification,
        F: 'static + Send + FnMut(T::Params, &mut AsyncApp),
    {
        Subscription::Detached
    }

    #[must_use]
    pub fn on_request<T, F, Fut>(&self, _f: F) -> Subscription
    where
        T: request::Request,
        T::Params: 'static + Send,
        F: 'static + FnMut(T::Params, &mut AsyncApp) -> Fut + Send,
        Fut: 'static + Future<Output = Result<T::Result>>,
    {
        Subscription::Detached
    }

    #[must_use]
    pub fn on_io<F>(&self, _f: F) -> Subscription
    where
        F: 'static + Send + FnMut(IoKind, &str),
    {
        Subscription::Detached
    }

    pub fn remove_request_handler<T: request::Request>(&self) {}

    pub fn remove_notification_handler<T: notification::Notification>(&self) {}

    pub fn has_notification_handler<T: notification::Notification>(&self) -> bool {
        false
    }

    pub fn name(&self) -> LanguageServerName {
        self.name.clone()
    }

    pub fn version(&self) -> Option<SharedString> {
        self.version.clone()
    }

    pub fn readable_version(&self) -> Option<SharedString> {
        self.version()
    }

    pub fn process_name(&self) -> &str {
        &self.process_name
    }

    pub fn capabilities(&self) -> ServerCapabilities {
        self.capabilities.read().clone()
    }

    pub fn adapter_server_capabilities(&self) -> AdapterServerCapabilities {
        AdapterServerCapabilities {
            server_capabilities: self.capabilities(),
            code_action_kinds: self.code_action_kinds(),
        }
    }

    pub fn update_capabilities(&self, update: impl FnOnce(&mut ServerCapabilities)) {
        update(&mut self.capabilities.write());
    }

    pub fn configuration(&self) -> &serde_json::Value {
        &self.configuration.settings
    }

    pub fn server_id(&self) -> LanguageServerId {
        self.server_id
    }

    pub fn process_id(&self) -> Option<u32> {
        None
    }

    pub fn binary(&self) -> &LanguageServerBinary {
        &self.binary
    }

    pub fn request<T: request::Request>(
        &self,
        _params: T::Params,
        _request_timeout: Duration,
    ) -> impl LspRequestFuture<T::Result> + use<T>
    where
        T::Result: 'static + Send,
    {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        LspRequest::new(id, ready(ConnectionResult::ConnectionReset))
    }

    pub fn request_with_timer<T: request::Request, U: Future<Output = String>>(
        &self,
        _params: T::Params,
        _timer: U,
    ) -> impl LspRequestFuture<T::Result> + use<T, U>
    where
        T::Result: 'static + Send,
    {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        LspRequest::new(id, ready(ConnectionResult::ConnectionReset))
    }

    pub fn request_timer(&self, timeout: Duration) -> impl Future<Output = String> {
        ready(format!("which took over {timeout:?}"))
    }

    pub fn notify<T: notification::Notification>(&self, _params: T::Params) -> Result<()> {
        Ok(())
    }

    pub fn add_workspace_folder(&self, uri: Uri) {
        if let Some(folders) = &self.workspace_folders {
            folders.lock().insert(uri);
        }
    }

    pub fn remove_workspace_folder(&self, uri: Uri) {
        if let Some(folders) = &self.workspace_folders {
            folders.lock().remove(&uri);
        }
    }

    pub fn set_workspace_folders(&self, folders: BTreeSet<Uri>) {
        if let Some(workspace_folders) = &self.workspace_folders {
            *workspace_folders.lock() = folders;
        }
    }

    pub fn workspace_folders(&self) -> BTreeSet<Uri> {
        if let Some(folders) = &self.workspace_folders {
            return folders.lock().clone();
        }

        self.root_uri
            .clone()
            .map(|root_uri| BTreeSet::from_iter([root_uri]))
            .unwrap_or_default()
    }

    pub fn register_buffer(
        &self,
        _uri: Uri,
        _language_id: String,
        _version: i32,
        _initial_text: String,
    ) {
    }

    pub fn unregister_buffer(&self, _uri: Uri) {}
}

impl Subscription {
    pub fn detach(&mut self) {}
}

impl fmt::Display for LanguageServerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Debug for LanguageServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LanguageServer")
            .field("id", &self.server_id.0)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for LanguageServerBinary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("LanguageServerBinary");
        debug.field("path", &self.path);
        debug.field("arguments", &self.arguments);

        if let Some(env) = &self.env {
            let redacted_env: BTreeMap<String, String> = env
                .iter()
                .map(|(key, value)| {
                    let redacted_value = if redact::should_redact(key) {
                        "REDACTED".to_string()
                    } else {
                        value.clone()
                    };
                    (key.clone(), redacted_value)
                })
                .collect();
            debug.field("env", &Some(redacted_env));
        } else {
            debug.field("env", &self.env);
        }

        debug.finish()
    }
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone)]
pub struct FakeLanguageServer {
    pub binary: LanguageServerBinary,
    pub server: Arc<LanguageServer>,
}

#[cfg(any(test, feature = "test-support"))]
impl FakeLanguageServer {
    pub fn new(
        server_id: LanguageServerId,
        binary: LanguageServerBinary,
        name: String,
        capabilities: ServerCapabilities,
        cx: &mut AsyncApp,
    ) -> (LanguageServer, FakeLanguageServer) {
        let server = LanguageServer::new(
            Arc::new(Mutex::new(None)),
            server_id,
            LanguageServerName(name.into()),
            binary.clone(),
            Path::new("/"),
            None,
            Some(Default::default()),
            cx,
        )
        .unwrap_or_else(|_| LanguageServer {
            server_id,
            next_id: AtomicI32::new(0),
            name: LanguageServerName::new_static("offline"),
            version: None,
            process_name: Arc::from("offline"),
            binary: binary.clone(),
            capabilities: Default::default(),
            configuration: Arc::new(DidChangeConfigurationParams {
                settings: serde_json::Value::Null,
            }),
            code_action_kinds: None,
            workspace_folders: Some(Default::default()),
            root_uri: None,
        });
        server.update_capabilities(|server_capabilities| {
            *server_capabilities = capabilities;
        });
        let fake = FakeLanguageServer {
            binary,
            server: Arc::new(server.clone_for_fake()),
        };
        (server, fake)
    }

    pub fn notify<T: notification::Notification>(&self, params: T::Params) {
        self.server.notify::<T>(params).ok();
    }

    pub async fn request<T>(
        &self,
        params: T::Params,
        timeout: Duration,
    ) -> ConnectionResult<T::Result>
    where
        T: request::Request,
        T::Result: 'static + Send,
    {
        self.server.request::<T>(params, timeout).await
    }

    pub async fn receive_notification<T: notification::Notification>(&mut self) -> T::Params {
        std::future::pending().await
    }

    pub async fn try_receive_notification<T: notification::Notification>(
        &mut self,
    ) -> Option<T::Params> {
        None
    }

    pub fn set_request_handler<T, F, Fut>(
        &self,
        _handler: F,
    ) -> futures::channel::mpsc::UnboundedReceiver<()>
    where
        T: 'static + request::Request,
        T::Params: 'static + Send,
        F: 'static + Send + FnMut(T::Params, gpui::AsyncApp) -> Fut,
        Fut: 'static + Future<Output = Result<T::Result>>,
    {
        let (_tx, rx) = futures::channel::mpsc::unbounded();
        rx
    }

    pub fn handle_notification<T, F>(
        &self,
        _handler: F,
    ) -> futures::channel::mpsc::UnboundedReceiver<()>
    where
        T: 'static + notification::Notification,
        T::Params: 'static + Send,
        F: 'static + Send + FnMut(T::Params, gpui::AsyncApp),
    {
        let (_tx, rx) = futures::channel::mpsc::unbounded();
        rx
    }

    pub fn remove_request_handler<T>(&mut self)
    where
        T: 'static + request::Request,
    {
    }

    pub async fn start_progress(&self, _token: impl Into<String>) {}

    pub async fn start_progress_with(
        &self,
        _token: impl Into<String>,
        _progress: WorkDoneProgressBegin,
        _request_timeout: Duration,
    ) {
    }

    pub fn end_progress(&self, _token: impl Into<String>) {}
}

#[cfg(any(test, feature = "test-support"))]
impl LanguageServer {
    fn clone_for_fake(&self) -> Self {
        Self {
            server_id: self.server_id,
            next_id: AtomicI32::new(self.next_id.load(Ordering::SeqCst)),
            name: self.name.clone(),
            version: self.version.clone(),
            process_name: self.process_name.clone(),
            binary: self.binary.clone(),
            capabilities: RwLock::new(self.capabilities()),
            configuration: self.configuration.clone(),
            code_action_kinds: self.code_action_kinds.clone(),
            workspace_folders: self.workspace_folders.clone(),
            root_uri: self.root_uri.clone(),
        }
    }
}
