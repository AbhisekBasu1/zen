pub mod telemetry;
pub mod user;

use anyhow::{Result, anyhow};
use futures::{Future, FutureExt as _, Stream, future::BoxFuture, stream};
use gpui::{App, AsyncApp, Global, actions};
use http_client::HttpClientWithUrl;
use parking_lot::Mutex;
use postage::watch;
use rpc::{
    ConnectionId as RpcConnectionId, ProtoClient as RpcProtoClient,
    ProtoMessageHandlerSet as RpcProtoMessageHandlerSet, TypedEnvelope as RpcTypedEnvelope,
    proto::{self as rpc_proto, EnvelopedMessage, PeerId, RequestMessage},
};
use std::{
    future::ready,
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use telemetry::Telemetry;
use util::ConnectionResult;

pub use rpc::*;
pub use user::*;

pub static IMPERSONATE_LOGIN: std::sync::LazyLock<Option<String>> =
    std::sync::LazyLock::new(|| None);

pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(1);

actions!(
    client,
    [
        /// Signs in to Zen account.
        SignIn,
        /// Signs out of Zen account.
        SignOut,
        /// Reconnects to the collaboration server.
        Reconnect
    ]
);

pub fn init(_: &Arc<Client>, _: &mut App) {}

struct GlobalClient(Arc<Client>);

impl Global for GlobalClient {}

pub struct Client {
    id: AtomicU64,
    http: Arc<HttpClientWithUrl>,
    status: watch::Receiver<Status>,
    handler_set: Mutex<RpcProtoMessageHandlerSet>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Status {
    SignedOut,
    UpgradeRequired,
    Authenticating,
    Authenticated,
    AuthenticationError,
    Connecting,
    ConnectionError,
    Connected {
        peer_id: PeerId,
        connection_id: RpcConnectionId,
    },
    ConnectionLost,
    Reauthenticating,
    Reauthenticated,
    Reconnecting,
    ReconnectionError {
        next_reconnection: Instant,
    },
}

impl Status {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }

    pub fn was_connected(&self) -> bool {
        matches!(
            self,
            Self::ConnectionLost
                | Self::Reauthenticating
                | Self::Reauthenticated
                | Self::Reconnecting
        )
    }

    pub fn is_or_was_connected(&self) -> bool {
        self.is_connected() || self.was_connected()
    }

    pub fn is_signing_in(&self) -> bool {
        matches!(
            self,
            Self::Authenticating | Self::Reauthenticating | Self::Connecting | Self::Reconnecting
        )
    }

    pub fn is_signed_out(&self) -> bool {
        matches!(self, Self::SignedOut | Self::UpgradeRequired)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Credentials {
    pub user_id: u64,
    pub access_token: String,
}

impl Credentials {
    pub fn authorization_header(&self) -> String {
        format!("{} {}", self.user_id, self.access_token)
    }
}

pub enum Subscription {
    Entity,
    Message,
}

pub struct PendingEntitySubscription<T: 'static> {
    _entity_type: PhantomData<T>,
}

impl<T: 'static> PendingEntitySubscription<T> {
    pub fn set_entity(self, _: &gpui::Entity<T>, _: &AsyncApp) -> Subscription {
        Subscription::Entity
    }
}

impl Client {
    pub fn new(http: Arc<HttpClientWithUrl>) -> Arc<Self> {
        let (_status_tx, status) = watch::channel_with(Status::SignedOut);
        Arc::new(Self {
            id: AtomicU64::new(0),
            http,
            status,
            handler_set: Mutex::default(),
        })
    }

    pub fn production(cx: &mut App) -> Arc<Self> {
        let http = Arc::new(HttpClientWithUrl::new_url(
            cx.http_client(),
            "https://zen.local",
            cx.http_client().proxy().cloned(),
        ));
        Self::new(http)
    }

    pub fn id(&self) -> u64 {
        self.id.load(Ordering::SeqCst)
    }

    pub fn http_client(&self) -> Arc<HttpClientWithUrl> {
        self.http.clone()
    }

    pub fn set_id(&self, id: u64) -> &Self {
        self.id.store(id, Ordering::SeqCst);
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn teardown(&self) {
        self.handler_set.lock().clear();
    }

    pub fn global(cx: &App) -> Arc<Self> {
        cx.global::<GlobalClient>().0.clone()
    }

    pub fn set_global(client: Arc<Client>, cx: &mut App) {
        cx.set_global(GlobalClient(client))
    }

    pub fn user_id(&self) -> Option<u64> {
        None
    }

    pub fn peer_id(&self) -> Option<PeerId> {
        None
    }

    pub fn status(&self) -> watch::Receiver<Status> {
        self.status.clone()
    }

    pub fn subscribe_to_entity<T>(self: &Arc<Self>, _: u64) -> Result<PendingEntitySubscription<T>>
    where
        T: 'static,
    {
        Ok(PendingEntitySubscription {
            _entity_type: PhantomData,
        })
    }

    pub fn add_message_handler<M, E, H, F>(
        self: &Arc<Self>,
        _: gpui::WeakEntity<E>,
        _: H,
    ) -> Subscription
    where
        M: EnvelopedMessage,
        E: 'static,
        H: 'static + Sync + Fn(gpui::Entity<E>, RpcTypedEnvelope<M>, AsyncApp) -> F + Send + Sync,
        F: 'static + Future<Output = Result<()>>,
    {
        Subscription::Message
    }

    pub fn add_request_handler<M, E, H, F>(
        self: &Arc<Self>,
        _: gpui::WeakEntity<E>,
        _: H,
    ) -> Subscription
    where
        M: RequestMessage,
        E: 'static,
        H: 'static + Sync + Fn(gpui::Entity<E>, RpcTypedEnvelope<M>, AsyncApp) -> F + Send + Sync,
        F: 'static + Future<Output = Result<M::Response>>,
    {
        Subscription::Message
    }

    pub async fn has_credentials(&self, _: &AsyncApp) -> bool {
        false
    }

    pub async fn sign_in(self: &Arc<Self>, _: bool, _: &AsyncApp) -> Result<()> {
        Ok(())
    }

    pub async fn sign_in_with_optional_connect(
        self: &Arc<Self>,
        _: bool,
        _: &AsyncApp,
    ) -> Result<()> {
        Ok(())
    }

    pub async fn connect(self: &Arc<Self>, _: bool, _: &AsyncApp) -> ConnectionResult<()> {
        ConnectionResult::Result(Err(anyhow!("collaboration is not available in this build")))
    }

    pub async fn sign_out(self: &Arc<Self>, _: &AsyncApp) {}

    pub fn request_sign_out(&self) {}

    pub fn disconnect(self: &Arc<Self>, _: &AsyncApp) {}

    pub fn reconnect(self: &Arc<Self>, _: &AsyncApp) {}

    pub fn send<T: EnvelopedMessage>(&self, _: T) -> Result<()> {
        Ok(())
    }

    pub fn request<T: RequestMessage>(
        &self,
        _: T,
    ) -> impl Future<Output = Result<T::Response>> + use<T> {
        ready(Err(anyhow!("collaboration is not available in this build")))
    }

    pub fn request_stream<T: RequestMessage>(
        &self,
        _: T,
    ) -> impl Future<Output = Result<impl Stream<Item = Result<T::Response>>>> {
        ready(Ok(stream::empty()))
    }

    pub fn request_envelope<T: RequestMessage>(
        &self,
        _: T,
    ) -> impl Future<Output = Result<RpcTypedEnvelope<T::Response>>> + use<T> {
        ready(Err(anyhow!("collaboration is not available in this build")))
    }

    pub fn request_dynamic(
        &self,
        _: rpc_proto::Envelope,
        _: &'static str,
    ) -> impl Future<Output = Result<rpc_proto::Envelope>> + use<> {
        ready(Err(anyhow!("collaboration is not available in this build")))
    }

    pub fn add_message_to_client_handler(
        self: &Arc<Client>,
        _: impl Fn(&(), &mut App) + Send + Sync + 'static,
    ) {
    }

    pub fn telemetry(&self) -> &'static Arc<Telemetry> {
        Telemetry::global()
    }
}

impl RpcProtoClient for Client {
    fn request(
        &self,
        _: rpc_proto::Envelope,
        _: &'static str,
    ) -> BoxFuture<'static, Result<rpc_proto::Envelope>> {
        async { Err(anyhow!("collaboration is not available in this build")) }.boxed()
    }

    fn send(&self, _: rpc_proto::Envelope, _: &'static str) -> Result<()> {
        Ok(())
    }

    fn send_response(&self, _: rpc_proto::Envelope, _: &'static str) -> Result<()> {
        Ok(())
    }

    fn message_handler_set(&self) -> &Mutex<RpcProtoMessageHandlerSet> {
        &self.handler_set
    }

    fn is_via_collab(&self) -> bool {
        false
    }

    fn has_wsl_interop(&self) -> bool {
        false
    }
}

pub const ZEN_URL_SCHEME: &str = "zen";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZenLink {
    Channel {
        channel_id: u64,
    },
    ChannelNotes {
        channel_id: u64,
        heading: Option<String>,
    },
}

pub fn parse_zen_link(_: &str, _: &App) -> Option<ZenLink> {
    None
}
