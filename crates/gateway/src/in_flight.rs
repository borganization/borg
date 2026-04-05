//! Per-session registry of in-flight agent turn cancellation tokens.
//!
//! When a gateway request triggers an agent turn, the turn's `CancellationToken`
//! is registered here keyed by `session_id`. A subsequent `/cancel` slash command
//! (or `POST /internal/cancel`) looks up the session's token and signals it,
//! causing the in-progress turn to exit cleanly at the next cancellation
//! checkpoint in `Agent::run_agent_loop`.
//!
//! Cleanup is guaranteed via the [`InFlightGuard`] RAII helper — even on panic
//! or early return, the guard drops the registry entry.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Process-wide in-flight registry shared by `handler::invoke_agent`, the
/// `/internal/cancel` HTTP endpoint, and the `/cancel` slash command handler.
pub static GLOBAL: LazyLock<InFlightRegistry> = LazyLock::new(InFlightRegistry::new);

/// Shared registry of active per-session cancellation tokens.
#[derive(Clone, Default)]
pub struct InFlightRegistry {
    inner: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl InFlightRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) the cancellation token for a session.
    ///
    /// If a token already exists for this session it is dropped without being
    /// cancelled — callers are expected to either use [`InFlightGuard`] for
    /// automatic cleanup or only register when no prior turn is running.
    pub async fn register(&self, session_id: &str, token: CancellationToken) {
        let mut map = self.inner.lock().await;
        map.insert(session_id.to_string(), token);
    }

    /// Cancel the in-flight turn for a specific session.
    ///
    /// Returns `true` if a token was found and cancelled, `false` if no turn
    /// was in flight for this session.
    pub async fn cancel(&self, session_id: &str) -> bool {
        let mut map = self.inner.lock().await;
        if let Some(token) = map.remove(session_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    /// Cancel every in-flight turn. Returns the number of turns cancelled.
    pub async fn cancel_all(&self) -> usize {
        let mut map = self.inner.lock().await;
        let count = map.len();
        for (_, token) in map.drain() {
            token.cancel();
        }
        count
    }

    /// Remove a session's entry without cancelling (used when a turn completes
    /// normally and no longer needs to be cancellable).
    pub async fn clear(&self, session_id: &str) {
        let mut map = self.inner.lock().await;
        map.remove(session_id);
    }

    /// Number of currently in-flight turns.
    pub async fn len(&self) -> usize {
        self.inner.lock().await.len()
    }

    /// Whether no turns are in flight.
    pub async fn is_empty(&self) -> bool {
        self.inner.lock().await.is_empty()
    }
}

/// RAII guard that clears the session's registry entry when dropped.
///
/// Ensures the registry doesn't leak entries if the caller returns early or
/// panics. Dropping the guard does **not** cancel — it only removes the entry,
/// so a completed turn will not linger.
pub struct InFlightGuard {
    registry: InFlightRegistry,
    session_id: String,
}

impl InFlightGuard {
    /// Register `token` for `session_id` and return a guard that will clear
    /// the entry on drop.
    pub async fn register(
        registry: InFlightRegistry,
        session_id: String,
        token: CancellationToken,
    ) -> Self {
        registry.register(&session_id, token).await;
        Self {
            registry,
            session_id,
        }
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        // We're in a sync Drop but the registry's Mutex is async. Spawn a
        // detached task to clear — entries are small and cleanup is best-effort.
        let registry = self.registry.clone();
        let session_id = std::mem::take(&mut self.session_id);
        tokio::spawn(async move {
            registry.clear(&session_id).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_then_cancel_returns_true() {
        let reg = InFlightRegistry::new();
        let token = CancellationToken::new();
        reg.register("sess-1", token.clone()).await;
        assert!(reg.cancel("sess-1").await);
        assert!(token.is_cancelled());
        // Second cancel on the same session is a no-op.
        assert!(!reg.cancel("sess-1").await);
    }

    #[tokio::test]
    async fn cancel_unknown_session_returns_false() {
        let reg = InFlightRegistry::new();
        assert!(!reg.cancel("never-registered").await);
    }

    #[tokio::test]
    async fn clear_removes_without_cancelling() {
        let reg = InFlightRegistry::new();
        let token = CancellationToken::new();
        reg.register("sess-1", token.clone()).await;
        reg.clear("sess-1").await;
        assert!(!token.is_cancelled());
        assert!(reg.is_empty().await);
    }

    #[tokio::test]
    async fn cancel_all_cancels_every_session() {
        let reg = InFlightRegistry::new();
        let t1 = CancellationToken::new();
        let t2 = CancellationToken::new();
        let t3 = CancellationToken::new();
        reg.register("a", t1.clone()).await;
        reg.register("b", t2.clone()).await;
        reg.register("c", t3.clone()).await;
        assert_eq!(reg.cancel_all().await, 3);
        assert!(t1.is_cancelled());
        assert!(t2.is_cancelled());
        assert!(t3.is_cancelled());
        assert!(reg.is_empty().await);
    }

    #[tokio::test]
    async fn register_replaces_previous_token() {
        let reg = InFlightRegistry::new();
        let old = CancellationToken::new();
        let new = CancellationToken::new();
        reg.register("sess", old.clone()).await;
        reg.register("sess", new.clone()).await;
        assert!(reg.cancel("sess").await);
        // Only the most recent token is cancelled; old one is simply dropped.
        assert!(new.is_cancelled());
        assert!(!old.is_cancelled());
    }

    #[tokio::test]
    async fn in_flight_guard_clears_on_drop() {
        let reg = InFlightRegistry::new();
        let token = CancellationToken::new();
        {
            let _guard =
                InFlightGuard::register(reg.clone(), "sess-guard".to_string(), token.clone()).await;
            assert_eq!(reg.len().await, 1);
        }
        // Drop spawns a detached cleanup task; yield until it runs.
        for _ in 0..100 {
            if reg.is_empty().await {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(reg.is_empty().await);
        // Guard drop does NOT cancel.
        assert!(!token.is_cancelled());
    }

    #[tokio::test]
    async fn concurrent_register_and_cancel_safe() {
        let reg = InFlightRegistry::new();
        let mut handles = Vec::new();
        for i in 0..20 {
            let reg = reg.clone();
            handles.push(tokio::spawn(async move {
                let token = CancellationToken::new();
                let key = format!("s{i}");
                reg.register(&key, token.clone()).await;
                reg.cancel(&key).await
            }));
        }
        for h in handles {
            assert!(h.await.unwrap());
        }
        assert!(reg.is_empty().await);
    }
}
