// Idle session pooling for the connector call proxy (doc 24 §5).
//
// The one-shot path costs a process start-up plus a handshake per call. This
// module keeps **stdio** sessions alive between calls so a repeat call costs a
// single `tools/call` round trip.
//
// ## Why only stdio
//
// Because we own it. A stdio server is a child process *we* spawned, so its
// lifetime is ours to decide, expire, and kill. A remote HTTP/SSE session id is
// expired by the *server* whenever it pleases (the spec's answer to a stale id
// is a 404), so caching one would convert a guaranteed-successful call into an
// occasionally-mysterious failure in exchange for one saved round trip — while
// `reqwest` already pools the TCP/TLS connection underneath. So remote
// transports stay one-shot: **pool what we own, do not cache what a remote
// server can expire behind our back.**
//
// ## What a pooled session is keyed by
//
// The connector **id** plus the configuration and credentials it was built
// from ([`StdioIdentity`]). The id — not the registered name — is the pin, for
// the same reason the allowlist accepts it: MCP servers upsert by name, so a
// later install can take a name over and would otherwise inherit a session
// opened for its predecessor. The identity is what makes an *edit* safe: change
// the command, the args, or a credential's value and the session no longer
// matches, so it is replaced rather than reused.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use super::session::{StdioCallError, StdioIdentity, StdioToolSession};
use super::{McpConnectionTestService, McpToolCallError, McpToolCallOutcome};
use crate::types::McpServerTransport;

/// How long a session may sit unused before it is closed.
const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(300);
/// Upper bound on live pooled server processes.
const DEFAULT_CAPACITY: usize = 8;

/// One pooled stdio server.
struct Pooled {
    identity: StdioIdentity,
    session: Arc<Mutex<StdioToolSession>>,
    last_used: Instant,
}

/// What [`McpToolCallPool::acquire`] decided to do.
enum Acquired {
    /// A session to call on (pooled, or freshly connected but unpooled).
    Session(Arc<Mutex<StdioToolSession>>),
    /// The pool is full of sessions that are in use; the caller should fall
    /// back to a one-shot call rather than wait.
    AtCapacity,
    /// Spawning or handshaking failed, and there is nothing better to try.
    Failed(String),
}

/// Executes connector calls, reusing live stdio sessions between them.
///
/// Pooling is an optimisation and never a correctness requirement: every path
/// here has a non-pooled fallback, and a session that cannot be trusted is
/// discarded rather than retried.
pub struct McpToolCallPool {
    service: McpConnectionTestService,
    idle_ttl: Duration,
    capacity: usize,
    sessions: Mutex<HashMap<String, Pooled>>,
}

impl McpToolCallPool {
    pub fn new(service: McpConnectionTestService) -> Self {
        Self {
            service,
            idle_ttl: DEFAULT_IDLE_TTL,
            capacity: DEFAULT_CAPACITY,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Override how long an unused session is kept (tests use a short TTL).
    pub fn with_idle_ttl(mut self, idle_ttl: Duration) -> Self {
        self.idle_ttl = idle_ttl;
        self
    }

    /// Override how many server processes may be pooled at once.
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.capacity = capacity;
        self
    }

    /// Call one tool, reusing a live session when the transport is stdio.
    pub async fn call(
        &self,
        connector_id: &str,
        transport: &McpServerTransport,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallOutcome, McpToolCallError> {
        self.call_for(connector_id, transport, tool, arguments, None).await
    }

    /// The same call on behalf of one principal (`34` §7).
    ///
    /// The principal reaches the session **through its env**: a pooled stdio
    /// session was spawned with somebody's resolved credentials, and the pool
    /// compares that resolved env, so two principals never share a session unless
    /// their credentials genuinely resolve to the same values.
    pub async fn call_for(
        &self,
        connector_id: &str,
        transport: &McpServerTransport,
        tool: &str,
        arguments: serde_json::Value,
        principal: Option<&str>,
    ) -> Result<McpToolCallOutcome, McpToolCallError> {
        let McpServerTransport::Stdio { command, args, env } = transport else {
            // Remote transports are the server's to expire; see the module doc.
            return self
                .service
                .call_tool_for(transport, tool, arguments, principal)
                .await;
        };
        let identity = StdioIdentity::new(command, args, env, principal);
        let budget = self.service.timeout;

        match tokio::time::timeout(
            budget,
            self.call_stdio(connector_id, transport, &identity, tool, arguments, principal),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                // A call that ran out of budget may still have a reply in
                // flight, so this pipe's request/response pairing can no longer
                // be trusted — the next call could read *this* call's answer.
                // The session is killed; the next call starts a fresh one.
                self.evict(connector_id).await;
                Err(McpToolCallError::Timeout(budget))
            }
        }
    }

    async fn call_stdio(
        &self,
        connector_id: &str,
        transport: &McpServerTransport,
        identity: &StdioIdentity,
        tool: &str,
        arguments: serde_json::Value,
        principal: Option<&str>,
    ) -> Result<McpToolCallOutcome, McpToolCallError> {
        let session = match self.acquire(connector_id, identity).await {
            Acquired::Session(session) => session,
            // At capacity with every session busy: a one-shot call still
            // answers the caller, it just pays the start-up again.
            Acquired::AtCapacity => {
                return self
                    .service
                    .call_tool_for(transport, tool, arguments, principal)
                    .await;
            }
            Acquired::Failed(message) => return Err(McpToolCallError::Failed(message)),
        };

        let mut session = session.lock().await;
        match session.call(tool, &arguments).await {
            Ok(reply) => Ok(McpToolCallOutcome { is_error: reply.is_error, result: reply.result }),
            Err(StdioCallError::Rejected(message)) => {
                // The server answered this call, so the session is still in
                // step and stays pooled.
                Err(McpToolCallError::Failed(message))
            }
            Err(StdioCallError::Broken(message)) => {
                drop(session);
                self.evict(connector_id).await;
                Err(McpToolCallError::Failed(message))
            }
        }
    }

    /// Find, or open, a session for `connector_id`.
    async fn acquire(&self, connector_id: &str, identity: &StdioIdentity) -> Acquired {
        let now = Instant::now();
        let mut stale: Vec<Pooled> = Vec::new();

        {
            let mut sessions = self.sessions.lock().await;
            stale.extend(sweep(&mut sessions, now, self.idle_ttl));

            match sessions.get_mut(connector_id) {
                // Same connector, same configuration and credentials: reuse.
                Some(pooled) if pooled.identity == *identity => {
                    pooled.last_used = now;
                    let session = pooled.session.clone();
                    drop(sessions);
                    self.reap(stale).await;
                    return Acquired::Session(session);
                }
                // Same connector, edited configuration or rotated credential:
                // the old session is stale and must go rather than linger.
                Some(_) => {
                    if let Some(pooled) = sessions.remove(connector_id) {
                        stale.push(pooled);
                    }
                }
                None => {}
            }

            if sessions.len() >= self.capacity {
                drop(sessions);
                self.reap(stale).await;
                return Acquired::AtCapacity;
            }
        }
        self.reap(stale).await;

        // Connected outside the map lock: an `npx` handshake can take seconds,
        // and holding the lock across it would serialize every other
        // connector's calls behind this one.
        let session = match StdioToolSession::connect(identity.clone()).await {
            Ok(session) => Arc::new(Mutex::new(session)),
            Err(error) => return Acquired::Failed(error),
        };

        let mut sessions = self.sessions.lock().await;
        // Another caller may have opened this connector's session while we were
        // connecting. Prefer theirs and let ours drop (which kills its child).
        if let Some(pooled) = sessions.get(connector_id)
            && pooled.identity == *identity
        {
            return Acquired::Session(pooled.session.clone());
        }
        if sessions.len() >= self.capacity {
            // Raced to full: hand the session over anyway, just unpooled.
            return Acquired::Session(session);
        }
        sessions.insert(
            connector_id.to_owned(),
            Pooled { identity: identity.clone(), session: session.clone(), last_used: Instant::now() },
        );
        Acquired::Session(session)
    }

    /// Drop `connector_id`'s session, killing its process tree.
    async fn evict(&self, connector_id: &str) {
        let removed = self.sessions.lock().await.remove(connector_id);
        if let Some(pooled) = removed {
            self.reap(vec![pooled]).await;
        }
    }

    /// Close every pooled session and empty the pool.
    pub async fn close_all(&self) {
        let all: Vec<Pooled> = self.sessions.lock().await.drain().map(|(_, pooled)| pooled).collect();
        self.reap(all).await;
    }

    /// Kill the process trees of discarded sessions.
    ///
    /// A session that is still mid-call is not waited for: blocking here would
    /// make an unrelated caller pay for a slow one. Its child is killed by
    /// `kill_on_drop` when the last handle goes, and the tree cleanup is handed
    /// to a detached task that waits its turn.
    async fn reap(&self, stale: Vec<Pooled>) {
        for pooled in stale {
            let session = pooled.session;
            if session.try_lock().is_err() {
                // Mid-call: hand the cleanup to a task that waits its turn.
                tokio::spawn(async move {
                    session.lock().await.close().await;
                });
            } else {
                session.lock().await.close().await;
            }
        }
    }
}

/// Remove sessions idle for at least `idle_ttl`.
fn sweep(sessions: &mut HashMap<String, Pooled>, now: Instant, idle_ttl: Duration) -> Vec<Pooled> {
    let expired: Vec<String> = sessions
        .iter()
        .filter(|(_, pooled)| now.duration_since(pooled.last_used) >= idle_ttl)
        .map(|(id, _)| id.clone())
        .collect();
    expired.into_iter().filter_map(|id| sessions.remove(&id)).collect()
}
