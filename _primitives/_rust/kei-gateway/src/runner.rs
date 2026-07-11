//! Orchestrates the full inbound → agent → delivery loop.
//!
//! Hermes equivalent: `gateway/run.py:_handle_message_event`.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::adapters::base::OutboundMessage;
use crate::agent_cache::{AgentCache, CachedAgent};
use crate::guard::SessionGuard;
use crate::message::MessageEvent;
use crate::router::{DeliveryRouter, DeliveryTarget};
use crate::session_key::{build_session_key, SessionKeyOpts};
use crate::session_store::SessionStore;

/// Outcome of one [`AgentRunFn::run`] call.
pub struct AgentRunOutcome {
    /// Outbound text. Empty string means "[SILENT] — no delivery".
    pub text: String,
    /// Warm handle to keep in the [`AgentCache`] for the next turn on this
    /// session, paired with a config signature used to detect staleness.
    /// `None` skips caching (e.g. one-shot / stateless implementations).
    pub warm: Option<(Arc<dyn std::any::Any + Send + Sync>, String)>,
}

impl AgentRunOutcome {
    /// Convenience constructor for implementations that don't cache.
    pub fn text_only(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            warm: None,
        }
    }
}

/// Type-erased agent runner.
///
/// Real impl supplies `Arc<dyn AgentRunFn>`; the gateway only sees the trait.
#[async_trait::async_trait]
pub trait AgentRunFn: Send + Sync {
    /// Process an inbound event and return the agent's outbound text plus an
    /// optional warm handle to cache. `cached` is the previous turn's warm
    /// handle for this session (via [`AgentCache`]), if still fresh — the
    /// implementation downcasts it through `Any` to reuse a live process
    /// instead of cold-starting.
    async fn run(
        &self,
        session_key: &str,
        event: &MessageEvent,
        cached: Option<Arc<dyn std::any::Any + Send + Sync>>,
    ) -> Result<AgentRunOutcome>;
}

/// Configuration for [`GatewayRunner`].
#[derive(Clone)]
pub struct RunnerConfig {
    pub session_opts: SessionKeyOpts,
    /// Channel buffer for inbound events.
    pub inbound_buffer: usize,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            session_opts: SessionKeyOpts::default(),
            inbound_buffer: 256,
        }
    }
}

/// Top-level gateway runtime.
///
/// Wires:
/// - inbound mpsc channel (adapters push events here)
/// - per-session [`SessionGuard`]
/// - [`AgentCache`] for warm agents
/// - [`SessionStore`] for durable session bookkeeping
/// - [`DeliveryRouter`] for outbound dispatch
pub struct GatewayRunner {
    config: RunnerConfig,
    guard: SessionGuard,
    agent_cache: AgentCache,
    sessions: SessionStore,
    router: DeliveryRouter,
    agent_runner: Arc<dyn AgentRunFn>,
}

impl GatewayRunner {
    pub fn new(
        config: RunnerConfig,
        guard: SessionGuard,
        agent_cache: AgentCache,
        sessions: SessionStore,
        router: DeliveryRouter,
        agent_runner: Arc<dyn AgentRunFn>,
    ) -> Self {
        Self {
            config,
            guard,
            agent_cache,
            sessions,
            router,
            agent_runner,
        }
    }

    /// Process a single inbound event end-to-end.
    pub async fn handle_inbound(&self, event: MessageEvent) -> Result<()> {
        let key = build_session_key(&event.source, self.config.session_opts);
        let _lock = self.guard.acquire(&key).await;
        let _data = self
            .sessions
            .get_or_create(&key, || format!("session_{}", chrono::Utc::now().timestamp()))
            .await?;
        let cached = self.agent_cache.get(&key).await;
        let outcome = self.agent_runner.run(&key, &event, cached).await?;
        if let Some((handle, signature)) = outcome.warm {
            self.agent_cache
                .put(&key, CachedAgent::new(handle, signature))
                .await;
        }
        self.sessions.record_turn(&key).await?;
        if outcome.text.is_empty() {
            return Ok(());
        }
        let target = origin_target(&event, &key);
        self.router
            .deliver(target, OutboundMessage::text(outcome.text))
            .await?;
        Ok(())
    }

    /// Spawn the inbound consume loop on the current Tokio runtime. Returns the
    /// sender half so adapters can push events.
    pub fn start(self: Arc<Self>) -> mpsc::Sender<MessageEvent> {
        let (tx, mut rx) = mpsc::channel::<MessageEvent>(self.config.inbound_buffer);
        let me = self.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let Err(e) = me.handle_inbound(event).await {
                    eprintln!("[kei-gateway] inbound failed: {e:#}");
                }
            }
        });
        tx
    }

    /// Helper for tests: drop the agent cache. Mostly for debug.
    pub async fn purge_idle_agents(&self) -> usize {
        self.agent_cache.evict_idle().await
    }
}

/// Build an [`DeliveryTarget::Origin`] from the inbound event source.
fn origin_target(event: &MessageEvent, fallback_key: &str) -> DeliveryTarget {
    DeliveryTarget::Origin {
        platform: event.source.platform,
        chat_id: event
            .source
            .chat_id
            .clone()
            .unwrap_or_else(|| fallback_key.to_string()),
        thread_id: event.source.thread_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;
    use crate::message::{Platform, SessionSource};

    /// Records whether each `run` call was handed a warm cache hit, and
    /// always offers a new warm handle back.
    struct CountingAgent {
        calls: AtomicUsize,
        cache_hits: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl AgentRunFn for CountingAgent {
        async fn run(
            &self,
            _session_key: &str,
            _event: &MessageEvent,
            cached: Option<Arc<dyn std::any::Any + Send + Sync>>,
        ) -> Result<AgentRunOutcome> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if cached.is_some() {
                self.cache_hits.fetch_add(1, Ordering::SeqCst);
            }
            Ok(AgentRunOutcome {
                text: String::new(),
                warm: Some((
                    Arc::new(()) as Arc<dyn std::any::Any + Send + Sync>,
                    "sig-v1".into(),
                )),
            })
        }
    }

    async fn build_runner(agent: Arc<CountingAgent>) -> GatewayRunner {
        // `:memory:` gives each pooled connection its own DB, so the schema
        // created on connection 1 is invisible to connection 2. A tempfile
        // keeps the pool's `max_connections(8)` pointed at one real DB.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("sessions.sqlite3");
        let sessions = SessionStore::open(db_path.to_str().unwrap(), 16)
            .await
            .unwrap();
        std::mem::forget(dir);
        GatewayRunner::new(
            RunnerConfig::default(),
            SessionGuard::new(),
            AgentCache::new(16, Duration::from_secs(60)),
            sessions,
            DeliveryRouter::new(),
            agent,
        )
    }

    #[tokio::test]
    async fn second_turn_on_same_session_hits_the_warm_cache() {
        let agent = Arc::new(CountingAgent {
            calls: AtomicUsize::new(0),
            cache_hits: AtomicUsize::new(0),
        });
        let runner = build_runner(agent.clone()).await;
        let source = SessionSource::dm(Platform::Telegram, "42");

        runner
            .handle_inbound(MessageEvent::new("hi", source.clone()))
            .await
            .unwrap();
        runner
            .handle_inbound(MessageEvent::new("again", source))
            .await
            .unwrap();

        assert_eq!(agent.calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            agent.cache_hits.load(Ordering::SeqCst),
            1,
            "first turn is a cold start, second turn must see the cached handle"
        );
        assert_eq!(runner.agent_cache.len().await, 1);
    }

    #[tokio::test]
    async fn distinct_sessions_do_not_share_the_cache() {
        let agent = Arc::new(CountingAgent {
            calls: AtomicUsize::new(0),
            cache_hits: AtomicUsize::new(0),
        });
        let runner = build_runner(agent.clone()).await;

        runner
            .handle_inbound(MessageEvent::new(
                "hi",
                SessionSource::dm(Platform::Telegram, "1"),
            ))
            .await
            .unwrap();
        runner
            .handle_inbound(MessageEvent::new(
                "hi",
                SessionSource::dm(Platform::Telegram, "2"),
            ))
            .await
            .unwrap();

        assert_eq!(agent.cache_hits.load(Ordering::SeqCst), 0);
        assert_eq!(runner.agent_cache.len().await, 2);
    }
}
