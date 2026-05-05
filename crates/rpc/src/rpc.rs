pub mod auth;
#[cfg(any(test, feature = "test-support"))]
mod conn;
#[cfg(any(test, feature = "test-support"))]
mod message_stream;
mod notification;
#[cfg(any(test, feature = "test-support"))]
mod peer;

#[cfg(any(test, feature = "test-support"))]
pub use conn::Connection;
pub use notification::*;
#[cfg(any(test, feature = "test-support"))]
pub use peer::*;
pub use proto;
#[cfg(not(any(test, feature = "test-support")))]
use proto::PeerId;
pub use proto::{Receipt, TypedEnvelope, error::*};
#[cfg(not(any(test, feature = "test-support")))]
use serde::Serialize;
#[cfg(not(any(test, feature = "test-support")))]
use std::fmt;
mod macros;

#[cfg(feature = "gpui")]
mod proto_client;
#[cfg(feature = "gpui")]
pub use proto_client::*;

pub const PROTOCOL_VERSION: u32 = 68;

#[cfg(not(any(test, feature = "test-support")))]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize)]
pub struct ConnectionId {
    pub owner_id: u32,
    pub id: u32,
}

#[cfg(not(any(test, feature = "test-support")))]
impl From<ConnectionId> for PeerId {
    fn from(id: ConnectionId) -> Self {
        PeerId {
            owner_id: id.owner_id,
            id: id.id,
        }
    }
}

#[cfg(not(any(test, feature = "test-support")))]
impl From<PeerId> for ConnectionId {
    fn from(peer_id: PeerId) -> Self {
        Self {
            owner_id: peer_id.owner_id,
            id: peer_id.id,
        }
    }
}

#[cfg(not(any(test, feature = "test-support")))]
impl fmt::Display for ConnectionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.owner_id, self.id)
    }
}
