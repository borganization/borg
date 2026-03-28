use std::time::Duration;

/// Default HTTP timeout for gateway API clients.
pub const GATEWAY_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Default message chunk size for platforms with ~4000 char limits.
pub const DEFAULT_MESSAGE_CHUNK_SIZE: usize = 4000;

/// Peer kind for direct (1:1) messages.
pub const PEER_KIND_DIRECT: &str = "direct";

/// Peer kind for group/channel messages.
pub const PEER_KIND_GROUP: &str = "group";
