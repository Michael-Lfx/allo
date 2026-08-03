//! Daily quota and MCP call accounting.

use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU32, AtomicUsize, Ordering},
    Mutex,
};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::evidence::atomic_json_write;
use crate::managed::{
    AuthorizedParallelCall, ManagedMcpCallControl, ManagedMcpCallError, ManagedMcpTool,
    ParallelCallRejection,
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuotaLedger {
    pub(crate) utc_date: String,
    pub(crate) used_calls: u32,
    pub(crate) cooldown_until_unix: Option<i64>,
}

#[derive(Debug, Default)]
struct GateState {
    stop_reason: Option<String>,
    retry_after_ms: Option<u64>,
    cooldown_until_unix: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotaLedgerError {
    Exhausted,
    Io,
}

/// A process-safe daily quota gate. It is called immediately before the
/// Remote adapter, after Local policy has decided that a remote call is
/// actually needed.
pub(crate) struct FileQuotaControl {
    path: PathBuf,
    daily_cap: u32,
    max_calls: u32,
    run_calls: AtomicU32,
    fetch_calls: AtomicU32,
    search_calls: AtomicU32,
    recovery_calls: AtomicU32,
    retry_limit_violations: AtomicUsize,
    sensitive_egress_violations: AtomicUsize,
    call_lock: Mutex<()>,
    state: Mutex<GateState>,
}

impl FileQuotaControl {
    pub(crate) fn new(path: PathBuf, daily_cap: u32, max_calls: u32) -> Self {
        Self {
            path,
            daily_cap,
            max_calls,
            run_calls: AtomicU32::new(0),
            fetch_calls: AtomicU32::new(0),
            search_calls: AtomicU32::new(0),
            recovery_calls: AtomicU32::new(0),
            retry_limit_violations: AtomicUsize::new(0),
            sensitive_egress_violations: AtomicUsize::new(0),
            call_lock: Mutex::new(()),
            state: Mutex::new(GateState::default()),
        }
    }

    pub(crate) fn actual_calls(&self) -> u32 {
        self.run_calls.load(Ordering::SeqCst)
    }

    pub(crate) fn fetch_calls(&self) -> u32 {
        self.fetch_calls.load(Ordering::SeqCst)
    }

    pub(crate) fn search_calls(&self) -> u32 {
        self.search_calls.load(Ordering::SeqCst)
    }

    pub(crate) fn recovery_calls(&self) -> u32 {
        self.recovery_calls.load(Ordering::SeqCst)
    }

    pub(crate) fn retry_limit_violations(&self) -> usize {
        self.retry_limit_violations.load(Ordering::SeqCst)
    }

    pub(crate) fn sensitive_egress_violations(&self) -> usize {
        self.sensitive_egress_violations.load(Ordering::SeqCst)
    }

    pub(crate) fn state(&self) -> (Option<String>, Option<u64>, Option<i64>) {
        let state = self.state.lock().expect("quota state lock poisoned");
        (
            state.stop_reason.clone(),
            state.retry_after_ms,
            state.cooldown_until_unix,
        )
    }

    fn set_stop(&self, reason: &str) {
        self.state
            .lock()
            .expect("quota state lock poisoned")
            .stop_reason = Some(reason.to_owned());
    }

    fn record_rate_limit(&self, retry_after: Option<Duration>) {
        let retry_after_ms = retry_after.map(|value| {
            value
                .as_millis()
                .min(u128::from(u64::MAX)) as u64
        });
        let now = Utc::now().timestamp();
        let cooldown = retry_after.map(|value| {
            let seconds = value.as_secs().saturating_add(u64::from(value.subsec_nanos() > 0));
            now.saturating_add(i64::try_from(seconds).unwrap_or(i64::MAX))
        });
        let _ = update_ledger(&self.path, self.daily_cap, |ledger| {
            ledger.cooldown_until_unix = cooldown;
            Ok(())
        });
        let mut state = self.state.lock().expect("quota state lock poisoned");
        state.stop_reason = Some("rate_limited".to_owned());
        state.retry_after_ms = retry_after_ms;
        state.cooldown_until_unix = cooldown;
    }

    fn reserve_quota(
        &self,
        tool: ManagedMcpTool,
        attempt: u8,
    ) -> Result<(), ManagedMcpCallError> {
        let _lock = self.call_lock.lock().expect("quota call lock poisoned");
        if self.run_calls.load(Ordering::SeqCst) >= self.max_calls {
            self.set_stop("quota_exhausted");
            return Err(ManagedMcpCallError::QuotaExhausted);
        }
        let now = Utc::now().timestamp();
        let result = update_ledger(&self.path, self.daily_cap, |ledger| {
            if ledger
                .cooldown_until_unix
                .is_some_and(|until| until > now)
            {
                return Err(QuotaLedgerError::Exhausted);
            }
            if ledger.used_calls >= self.daily_cap {
                return Err(QuotaLedgerError::Exhausted);
            }
            ledger.used_calls = ledger.used_calls.saturating_add(1);
            Ok(())
        });
        match result {
            Ok(()) => {
                self.run_calls.fetch_add(1, Ordering::SeqCst);
                match tool {
                    ManagedMcpTool::Fetch => {
                        self.fetch_calls.fetch_add(1, Ordering::SeqCst);
                    }
                    ManagedMcpTool::Search => {
                        self.search_calls.fetch_add(1, Ordering::SeqCst);
                    }
                }
                if attempt > 1 {
                    self.recovery_calls.fetch_add(1, Ordering::SeqCst);
                }
                Ok(())
            }
            Err(QuotaLedgerError::Exhausted) => {
                if self.state().0.as_deref() != Some("rate_limited") {
                    self.set_stop("quota_exhausted");
                }
                Err(ManagedMcpCallError::QuotaExhausted)
            }
            Err(_) => {
                self.set_stop("quota_ledger_failed");
                Err(ManagedMcpCallError::LedgerFailure)
            }
        }
    }
}

#[async_trait]
impl ManagedMcpCallControl for FileQuotaControl {
    async fn reserve(
        &self,
        call: &AuthorizedParallelCall,
    ) -> Result<(), crate::managed::ManagedMcpControlError> {
        self.reserve_quota(call.tool(), call.attempt())
            .map_err(|error| match error {
                ManagedMcpCallError::QuotaExhausted => {
                    crate::managed::ManagedMcpControlError::QuotaExhausted
                }
                ManagedMcpCallError::LedgerFailure => {
                    crate::managed::ManagedMcpControlError::LedgerFailure
                }
                _ => crate::managed::ManagedMcpControlError::LedgerFailure,
            })
    }

    fn observe_rejection(&self, rejection: ParallelCallRejection) {
        match rejection {
            ParallelCallRejection::RetryLimitExceeded => {
                self.retry_limit_violations.fetch_add(1, Ordering::SeqCst);
                self.set_stop("safety_violation");
            }
            ParallelCallRejection::UnsafeArguments => {
                self.sensitive_egress_violations
                    .fetch_add(1, Ordering::SeqCst);
                self.set_stop("safety_violation");
            }
        }
    }

    fn observe_result(
        &self,
        _call: &AuthorizedParallelCall,
        result: &Result<nomi_mcp::protocol::McpToolResult, nomi_mcp::remote_peer::McpPeerError>,
    ) {
        if let Err(nomi_mcp::remote_peer::McpPeerError::Http {
            status,
            retry_after,
        }) = result
            && *status == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            self.record_rate_limit(*retry_after);
        }
    }
}

fn update_ledger<T>(
    path: &Path,
    daily_cap: u32,
    update: impl FnOnce(&mut QuotaLedger) -> Result<T, QuotaLedgerError>,
) -> Result<T, QuotaLedgerError> {
    if let Some(parent) = path.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|_| QuotaLedgerError::Io)?;
    }
    let lock_path = PathBuf::from(format!("{}.lock", path.display()));
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(lock_path)
        .map_err(|_| QuotaLedgerError::Io)?;
    lock.lock_exclusive()
        .map_err(|_| QuotaLedgerError::Io)?;
    let result = (|| {
        let mut ledger = if path.exists() {
            let mut file = File::open(path).map_err(|_| QuotaLedgerError::Io)?;
            read_locked_ledger(&mut file)?
        } else {
            QuotaLedger::default()
        };
        let today = Utc::now().date_naive().to_string();
        if ledger.utc_date != today {
            ledger = QuotaLedger {
                utc_date: today,
                used_calls: 0,
                cooldown_until_unix: None,
            };
        }
        let output = update(&mut ledger)?;
        if ledger.used_calls > daily_cap {
            return Err(QuotaLedgerError::Exhausted);
        }
        atomic_json_write(path, &ledger).map_err(|_| QuotaLedgerError::Io)?;
        Ok(output)
    })();
    let _ = lock.unlock();
    result
}

fn read_locked_ledger(file: &mut File) -> Result<QuotaLedger, QuotaLedgerError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| QuotaLedgerError::Io)?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(file, &mut bytes)
        .map_err(|_| QuotaLedgerError::Io)?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(QuotaLedgerError::Io);
    }
    serde_json::from_slice(&bytes).map_err(|_| QuotaLedgerError::Io)
}
