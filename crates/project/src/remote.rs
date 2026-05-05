use std::collections::BTreeMap;

pub use remote_client::{
    CommandTemplate, ConnectionIdentifier, ConnectionState, Interactive, MAX_RECONNECT_ATTEMPTS,
    RemoteArch, RemoteClient, RemoteClientDelegate, RemoteClientEvent, RemoteConnection,
    RemoteConnectionOptions, RemoteOs, RemotePlatform, connect, has_active_connection,
};
pub use settings::SshPortForwardOption;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SshConnectionHost {
    IpAddr(std::net::IpAddr),
    Hostname(String),
}

impl SshConnectionHost {
    pub fn to_bracketed_string(&self) -> String {
        match self {
            Self::IpAddr(std::net::IpAddr::V4(address)) => address.to_string(),
            Self::IpAddr(std::net::IpAddr::V6(address)) => format!("[{address}]"),
            Self::Hostname(hostname) => {
                if hostname.contains(':') && !hostname.starts_with('[') {
                    format!("[{hostname}]")
                } else {
                    hostname.clone()
                }
            }
        }
    }
}

impl std::fmt::Display for SshConnectionHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IpAddr(address) => write!(formatter, "{address}"),
            Self::Hostname(hostname) => formatter.write_str(hostname),
        }
    }
}

impl From<&str> for SshConnectionHost {
    fn from(value: &str) -> Self {
        value
            .parse()
            .map(Self::IpAddr)
            .unwrap_or_else(|_| Self::Hostname(value.to_string()))
    }
}

impl From<String> for SshConnectionHost {
    fn from(value: String) -> Self {
        value
            .parse()
            .map(Self::IpAddr)
            .unwrap_or(Self::Hostname(value))
    }
}

impl Default for SshConnectionHost {
    fn default() -> Self {
        Self::Hostname(String::new())
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SshConnectionOptions {
    pub host: SshConnectionHost,
    pub username: Option<String>,
    pub port: Option<u16>,
    pub password: Option<String>,
    pub args: Option<Vec<String>>,
    pub port_forwards: Option<Vec<SshPortForwardOption>>,
    pub connection_timeout: Option<u16>,
    pub nickname: Option<String>,
    pub upload_binary_over_ssh: bool,
}

impl SshConnectionOptions {
    pub fn connection_string(&self) -> String {
        let mut connection = String::new();
        if let Some(username) = self
            .username
            .as_deref()
            .filter(|username| !username.is_empty())
        {
            connection.push_str(username);
            connection.push('@');
        }
        connection.push_str(&self.host.to_bracketed_string());
        if let Some(port) = self.port {
            connection.push(':');
            connection.push_str(&port.to_string());
        }
        connection
    }

    pub fn parse_command_line(input: &str) -> anyhow::Result<Self> {
        let input = input.trim();
        if input.is_empty() {
            anyhow::bail!("SSH host cannot be empty");
        }

        let (username, host_and_port) = input
            .rsplit_once('@')
            .map(|(username, host)| (Some(username.to_string()), host))
            .unwrap_or((None, input));

        let (host, port) = parse_host_and_port(host_and_port)?;

        Ok(Self {
            host: host.into(),
            username,
            port,
            ..Default::default()
        })
    }
}

impl From<settings::SshConnection> for SshConnectionOptions {
    fn from(value: settings::SshConnection) -> Self {
        Self {
            host: value.host.to_string().into(),
            username: value.username,
            port: value.port,
            password: None,
            args: Some(value.args),
            port_forwards: value.port_forwards,
            connection_timeout: value.connection_timeout,
            nickname: value.nickname,
            upload_binary_over_ssh: value.upload_binary_over_ssh.unwrap_or_default(),
        }
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct WslConnectionOptions {
    pub distro_name: String,
    pub user: Option<String>,
}

impl From<settings::WslConnection> for WslConnectionOptions {
    fn from(value: settings::WslConnection) -> Self {
        Self {
            distro_name: value.distro_name,
            user: value.user,
        }
    }
}

impl WslConnectionOptions {
    pub fn abs_windows_path_to_wsl_path(
        &self,
        source: &std::path::Path,
    ) -> impl std::future::Future<Output = anyhow::Result<String>> + use<> {
        let path = source.to_string_lossy().replace('\\', "/");
        std::future::ready(Ok(path))
    }
}

#[derive(
    Debug,
    Default,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct DockerConnectionOptions {
    pub name: String,
    pub container_id: String,
    pub remote_user: String,
    pub upload_binary_over_docker_exec: bool,
    pub use_podman: bool,
    pub remote_env: BTreeMap<String, String>,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MockConnectionOptions {
    pub id: u64,
}

#[cfg(any(test, feature = "test-support"))]
pub struct MockConnection;

#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct MockConnectionRegistry;

#[cfg(any(test, feature = "test-support"))]
pub struct MockDelegate;

#[cfg(any(test, feature = "test-support"))]
pub type ConnectGuard = futures::channel::oneshot::Sender<()>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RemoteConnectionIdentity {
    Ssh {
        host: String,
        username: Option<String>,
        port: Option<u16>,
    },
    Wsl {
        distro_name: String,
        user: Option<String>,
    },
    Docker {
        container_id: String,
        name: String,
        remote_user: String,
    },
    #[cfg(any(test, feature = "test-support"))]
    Mock { id: u64 },
}

impl From<&RemoteConnectionOptions> for RemoteConnectionIdentity {
    fn from(options: &RemoteConnectionOptions) -> Self {
        match options {
            RemoteConnectionOptions::Ssh(options) => Self::Ssh {
                host: options.host.to_string(),
                username: options.username.clone(),
                port: options.port,
            },
            RemoteConnectionOptions::Wsl(options) => Self::Wsl {
                distro_name: options.distro_name.clone(),
                user: options.user.clone(),
            },
            RemoteConnectionOptions::Docker(options) => Self::Docker {
                container_id: options.container_id.clone(),
                name: options.name.clone(),
                remote_user: options.remote_user.clone(),
            },
            #[cfg(any(test, feature = "test-support"))]
            RemoteConnectionOptions::Mock(options) => Self::Mock { id: options.id },
        }
    }
}

pub fn remote_connection_identity(options: &RemoteConnectionOptions) -> RemoteConnectionIdentity {
    options.into()
}

pub fn same_remote_connection_identity(
    left: Option<&RemoteConnectionOptions>,
    right: Option<&RemoteConnectionOptions>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            remote_connection_identity(left) == remote_connection_identity(right)
        }
        (None, None) => true,
        _ => false,
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, PartialEq, Eq, gpui::Action)]
#[action(namespace = workspace, no_json, no_register)]
pub struct OpenWslPath {
    pub distro: WslConnectionOptions,
    pub paths: Vec<std::path::PathBuf>,
}

#[cfg(target_os = "windows")]
pub async fn wsl_path_to_windows_path(
    _: &WslConnectionOptions,
    _: &std::path::Path,
) -> anyhow::Result<std::path::PathBuf> {
    anyhow::bail!("remote development is not available in this build")
}

fn parse_host_and_port(input: &str) -> anyhow::Result<(&str, Option<u16>)> {
    if let Some(end) = input
        .strip_prefix('[')
        .and_then(|input| input.find(']').map(|end| end + 1))
    {
        let host = &input[1..end];
        let remainder = &input[end + 1..];
        let port = remainder
            .strip_prefix(':')
            .map(str::parse)
            .transpose()
            .map_err(|error| anyhow::anyhow!("invalid SSH port: {error}"))?;
        return Ok((host, port));
    }

    match input.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            let port = port
                .parse()
                .map_err(|error| anyhow::anyhow!("invalid SSH port: {error}"))?;
            Ok((host, Some(port)))
        }
        _ => Ok((input, None)),
    }
}

pub mod remote_client {
    #[cfg(any(test, feature = "test-support"))]
    use super::{ConnectGuard, MockConnectionOptions};
    use super::{DockerConnectionOptions, SshConnectionOptions, WslConnectionOptions};
    use anyhow::{Result, anyhow};
    use collections::HashMap;
    use futures::{FutureExt as _, channel::oneshot, future::BoxFuture};
    use gpui::{
        App, AppContext as _, AsyncApp, BackgroundExecutor, Context, Entity, EventEmitter, Task,
    };
    use parking_lot::Mutex;
    use rpc::{AnyProtoClient, ProtoClient, ProtoMessageHandlerSet, proto};
    use std::{
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };
    use util::paths::{PathStyle, RemotePathBuf};

    pub const MAX_RECONNECT_ATTEMPTS: usize = 3;

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum RemoteOs {
        Linux,
        MacOs,
        Windows,
    }

    impl RemoteOs {
        pub fn as_str(&self) -> &'static str {
            match self {
                Self::Linux => "linux",
                Self::MacOs => "macos",
                Self::Windows => "windows",
            }
        }

        pub fn is_windows(&self) -> bool {
            matches!(self, Self::Windows)
        }
    }

    impl std::fmt::Display for RemoteOs {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.as_str())
        }
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum RemoteArch {
        X86_64,
        Aarch64,
    }

    impl RemoteArch {
        pub fn as_str(&self) -> &'static str {
            match self {
                Self::X86_64 => "x86_64",
                Self::Aarch64 => "aarch64",
            }
        }
    }

    impl std::fmt::Display for RemoteArch {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.as_str())
        }
    }

    #[derive(Copy, Clone, Debug)]
    pub struct RemotePlatform {
        pub os: RemoteOs,
        pub arch: RemoteArch,
    }

    #[derive(Clone, Debug)]
    pub struct CommandTemplate {
        pub program: String,
        pub args: Vec<String>,
        pub env: HashMap<String, String>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Interactive {
        Yes,
        No,
    }

    pub trait RemoteClientDelegate: Send + Sync {
        fn set_status(&self, status: Option<&str>, cx: &mut AsyncApp);
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ConnectionState {
        Connecting,
        Connected,
        HeartbeatMissed,
        Reconnecting,
        Disconnected,
    }

    pub struct RemoteClient {
        client: Arc<OfflineProtoClient>,
        connection_options: RemoteConnectionOptions,
        path_style: PathStyle,
        connection_state: ConnectionState,
    }

    #[derive(Debug)]
    pub enum RemoteClientEvent {
        Disconnected { server_not_running: bool },
    }

    impl EventEmitter<RemoteClientEvent> for RemoteClient {}

    pub enum ConnectionIdentifier {
        Setup(u64),
        Workspace(i64),
    }

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    impl ConnectionIdentifier {
        pub fn setup() -> Self {
            Self::Setup(NEXT_ID.fetch_add(1, Ordering::SeqCst))
        }
    }

    pub async fn connect(
        _: RemoteConnectionOptions,
        delegate: Arc<dyn RemoteClientDelegate>,
        cx: &mut AsyncApp,
    ) -> Result<Arc<dyn RemoteConnection>> {
        delegate.set_status(
            Some("Remote development is not available in this build"),
            cx,
        );
        Err(anyhow!("remote development is not available in this build"))
    }

    pub fn has_active_connection(_: &RemoteConnectionOptions, _: &App) -> bool {
        false
    }

    impl RemoteClient {
        pub fn new(
            _: ConnectionIdentifier,
            remote_connection: Arc<dyn RemoteConnection>,
            _: oneshot::Receiver<()>,
            _: Arc<dyn RemoteClientDelegate>,
            cx: &mut App,
        ) -> Task<Result<Option<Entity<Self>>>> {
            let connection_options = remote_connection.connection_options();
            let path_style = remote_connection.path_style();
            let client = cx.new(|_| Self {
                client: OfflineProtoClient::new(),
                connection_options,
                path_style,
                connection_state: ConnectionState::Disconnected,
            });
            Task::ready(Ok(Some(client)))
        }

        pub fn shutdown_processes<T: rpc::proto::RequestMessage>(
            &mut self,
            _: Option<T>,
            _: BackgroundExecutor,
        ) -> Option<impl std::future::Future<Output = ()> + use<T>> {
            None::<std::future::Ready<()>>
        }

        pub fn shell(&self) -> Option<String> {
            None
        }

        pub fn default_system_shell(&self) -> Option<String> {
            None
        }

        pub fn shares_network_interface(&self) -> bool {
            false
        }

        pub fn build_command(
            &self,
            _: Option<String>,
            _: &[String],
            _: &HashMap<String, String>,
            _: Option<String>,
            _: Option<(u16, String, u16)>,
        ) -> Result<CommandTemplate> {
            Err(anyhow!("remote development is not available in this build"))
        }

        pub fn build_command_with_options(
            &self,
            _: Option<String>,
            _: &[String],
            _: &HashMap<String, String>,
            _: Option<String>,
            _: Option<(u16, String, u16)>,
            _: Interactive,
        ) -> Result<CommandTemplate> {
            Err(anyhow!("remote development is not available in this build"))
        }

        pub fn build_forward_ports_command(
            &self,
            _: Vec<(u16, String, u16)>,
        ) -> Result<CommandTemplate> {
            Err(anyhow!("remote development is not available in this build"))
        }

        pub fn upload_directory(&self, _: PathBuf, _: RemotePathBuf, _: &App) -> Task<Result<()>> {
            Task::ready(Err(anyhow!(
                "remote development is not available in this build"
            )))
        }

        pub fn proto_client(&self) -> AnyProtoClient {
            self.client.clone().into()
        }

        pub fn connection_options(&self) -> RemoteConnectionOptions {
            self.connection_options.clone()
        }

        pub fn connection(&self) -> Option<Arc<dyn RemoteConnection>> {
            None
        }

        pub fn connection_state(&self) -> ConnectionState {
            self.connection_state
        }

        pub fn is_disconnected(&self) -> bool {
            true
        }

        pub fn has_wsl_interop(&self) -> bool {
            false
        }

        pub fn path_style(&self) -> PathStyle {
            self.path_style
        }

        pub fn force_disconnect(&mut self, cx: &mut Context<Self>) -> Task<Result<()>> {
            self.connection_state = ConnectionState::Disconnected;
            cx.emit(RemoteClientEvent::Disconnected {
                server_not_running: false,
            });
            cx.notify();
            Task::ready(Ok(()))
        }

        pub fn force_heartbeat_timeout(&mut self, _: usize, cx: &mut Context<Self>) {
            self.connection_state = ConnectionState::Disconnected;
            cx.emit(RemoteClientEvent::Disconnected {
                server_not_running: false,
            });
            cx.notify();
        }

        pub fn force_server_not_running(&mut self, cx: &mut Context<Self>) {
            self.connection_state = ConnectionState::Disconnected;
            cx.emit(RemoteClientEvent::Disconnected {
                server_not_running: true,
            });
            cx.notify();
        }

        pub fn simulate_disconnect(&self, _: &mut App) -> Task<()> {
            Task::ready(())
        }

        pub fn remote_connection(&self) -> Option<Arc<dyn RemoteConnection>> {
            None
        }

        #[cfg(any(test, feature = "test-support"))]
        pub fn fake_server(
            _: &mut gpui::TestAppContext,
            _: &mut gpui::TestAppContext,
        ) -> (RemoteConnectionOptions, AnyProtoClient, ConnectGuard) {
            let (tx, _rx) = oneshot::channel();
            (
                RemoteConnectionOptions::Mock(MockConnectionOptions { id: 0 }),
                OfflineProtoClient::new().into(),
                tx,
            )
        }

        #[cfg(any(test, feature = "test-support"))]
        pub fn fake_server_with_opts(
            _: &RemoteConnectionOptions,
            _: &mut gpui::TestAppContext,
            _: &mut gpui::TestAppContext,
        ) -> (AnyProtoClient, ConnectGuard) {
            let (tx, _rx) = oneshot::channel();
            (OfflineProtoClient::new().into(), tx)
        }

        #[cfg(any(test, feature = "test-support"))]
        pub async fn connect_mock(
            opts: RemoteConnectionOptions,
            cx: &mut gpui::TestAppContext,
        ) -> Entity<Self> {
            let path_style = PathStyle::local();
            cx.update(|cx| {
                cx.new(|_| Self {
                    client: OfflineProtoClient::new(),
                    connection_options: opts,
                    path_style,
                    connection_state: ConnectionState::Disconnected,
                })
            })
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    pub enum RemoteConnectionOptions {
        Ssh(SshConnectionOptions),
        Wsl(WslConnectionOptions),
        Docker(DockerConnectionOptions),
        #[cfg(any(test, feature = "test-support"))]
        Mock(MockConnectionOptions),
    }

    impl RemoteConnectionOptions {
        pub fn display_name(&self) -> String {
            match self {
                Self::Ssh(options) => options
                    .nickname
                    .clone()
                    .unwrap_or_else(|| options.host.to_string()),
                Self::Wsl(options) => options.distro_name.clone(),
                Self::Docker(options) => {
                    if options.use_podman {
                        format!("[podman] {}", options.name)
                    } else {
                        options.name.clone()
                    }
                }
                #[cfg(any(test, feature = "test-support"))]
                Self::Mock(options) => format!("mock-{}", options.id),
            }
        }
    }

    impl From<SshConnectionOptions> for RemoteConnectionOptions {
        fn from(options: SshConnectionOptions) -> Self {
            Self::Ssh(options)
        }
    }

    impl From<WslConnectionOptions> for RemoteConnectionOptions {
        fn from(options: WslConnectionOptions) -> Self {
            Self::Wsl(options)
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    impl From<MockConnectionOptions> for RemoteConnectionOptions {
        fn from(options: MockConnectionOptions) -> Self {
            Self::Mock(options)
        }
    }

    pub trait RemoteConnection: Send + Sync {
        fn upload_directory(&self, _: PathBuf, _: RemotePathBuf, _: &App) -> Task<Result<()>>;
        fn kill(&self) -> Task<Result<()>> {
            Task::ready(Ok(()))
        }
        fn has_been_killed(&self) -> bool {
            true
        }
        fn shares_network_interface(&self) -> bool {
            false
        }
        fn build_command(
            &self,
            _: Option<String>,
            _: &[String],
            _: &HashMap<String, String>,
            _: Option<String>,
            _: Option<(u16, String, u16)>,
            _: Interactive,
        ) -> Result<CommandTemplate>;
        fn build_forward_ports_command(
            &self,
            _: Vec<(u16, String, u16)>,
        ) -> Result<CommandTemplate>;
        fn connection_options(&self) -> RemoteConnectionOptions;
        fn path_style(&self) -> PathStyle;
        fn shell(&self) -> String {
            String::new()
        }
        fn default_system_shell(&self) -> String {
            String::new()
        }
        fn has_wsl_interop(&self) -> bool {
            false
        }

        #[cfg(any(test, feature = "test-support"))]
        fn simulate_disconnect(&self, _: &AsyncApp) {}
    }

    struct OfflineProtoClient {
        handler_set: Mutex<ProtoMessageHandlerSet>,
    }

    impl OfflineProtoClient {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                handler_set: Mutex::default(),
            })
        }
    }

    impl ProtoClient for OfflineProtoClient {
        fn request(
            &self,
            _: proto::Envelope,
            _: &'static str,
        ) -> BoxFuture<'static, Result<proto::Envelope>> {
            async { Err(anyhow!("remote development is not available in this build")) }.boxed()
        }

        fn send(&self, _: proto::Envelope, _: &'static str) -> Result<()> {
            Ok(())
        }

        fn send_response(&self, _: proto::Envelope, _: &'static str) -> Result<()> {
            Ok(())
        }

        fn message_handler_set(&self) -> &Mutex<ProtoMessageHandlerSet> {
            &self.handler_set
        }

        fn is_via_collab(&self) -> bool {
            false
        }

        fn has_wsl_interop(&self) -> bool {
            false
        }
    }
}
