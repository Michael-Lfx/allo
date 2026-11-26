//! Shares launch capacity and workspace claims across foreground and background.

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flowy_agent_core::CancelToken;
use tokio::sync::watch;
use tokio::time::Instant;

const MAX_ACTIVE: usize = 2;
const MAX_BACKGROUND: usize = 1;
const PRIORITY_MAX_BYPASSES: u8 = 3;
const PRIORITY_MAX_WAIT: Duration = Duration::from_secs(30);
pub const EXECUTION_MARKER: &str = "execution-in-progress.lock";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionClass {
    Foreground,
    Background,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceAccess {
    Read,
    Write,
}

#[derive(Debug)]
pub struct WorkspaceClaim {
    directory: PhysicalDirectory,
    access: WorkspaceAccess,
}

impl WorkspaceClaim {
    pub(crate) fn resolve(
        directory: &Path,
        access: WorkspaceAccess,
    ) -> io::Result<Self> {
        Ok(Self {
            directory: PhysicalDirectory::resolve(directory)?,
            access,
        })
    }

    fn conflicts_with(&self, other: &Self) -> bool {
        (self.access == WorkspaceAccess::Write
            || other.access == WorkspaceAccess::Write)
            && (self.directory.covers(&other.directory)
                || other.directory.covers(&self.directory))
    }

    fn covers(&self, other: &Self) -> bool {
        (self.access == WorkspaceAccess::Write || self.access == other.access)
            && self.directory.covers(&other.directory)
    }
}

#[derive(Debug)]
struct PhysicalDirectory {
    path: PathBuf,
    #[cfg(unix)]
    line: Vec<FileIdentity>,
}

#[cfg(unix)]
type FileIdentity = (u64, u64);

impl PhysicalDirectory {
    fn resolve(path: &Path) -> io::Result<Self> {
        let canonical = path.canonicalize()?;
        if !fs::metadata(&canonical)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "workspace resource is not a directory",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            // Directory identities also resolve spelling aliases on
            // case-insensitive volumes and aliases created by mount points.
            let line = canonical
                .ancestors()
                .map(|ancestor| {
                    let metadata = fs::metadata(ancestor)?;
                    Ok((metadata.dev(), metadata.ino()))
                })
                .collect::<io::Result<Vec<_>>>()?;
            Ok(Self {
                path: canonical,
                line,
            })
        }
        #[cfg(windows)]
        {
            // Windows canonicalization resolves junctions and short names.
            // Fold spelling case before the component-wise ancestor check.
            let path =
                PathBuf::from(canonical.to_string_lossy().to_lowercase());
            Ok(Self { path })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Ok(Self { path: canonical })
        }
    }

    fn covers(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            other.path.starts_with(&self.path)
                || self
                    .line
                    .first()
                    .is_some_and(|identity| other.line.contains(identity))
        }
        #[cfg(not(unix))]
        {
            other.path.starts_with(&self.path)
        }
    }
}

#[derive(Debug)]
pub enum WorkspaceRequest {
    Scoped(Vec<WorkspaceClaim>),
    DeviceWrite,
}

impl WorkspaceRequest {
    /// Derive claims from the prepared registry after host policy filters.
    /// A generic shell receives the device claim because its writes cannot
    /// be proved confined to the session directory.
    pub(crate) fn for_tools(
        directory: &Path,
        tools: &[String],
    ) -> io::Result<Self> {
        // Router orchestration and automation bridge calls do not directly
        // touch the local workspace; their child work acquires its own permit.
        let non_filesystem = |tool: &str| {
            matches!(
                tool,
                "query_work_status"
                    | "dispatch_work"
                    | "control_work"
                    | "ask_user_choice"
            ) || tool.starts_with("automation_")
        };
        if tools.iter().all(|tool| non_filesystem(tool)) {
            return Ok(Self::Scoped(Vec::new()));
        }
        // Only these built-in file tools have path guards proving a scope.
        // Shell, verification/rendering, publishing and future unknown tools
        // are deliberately device-exclusive until all of their I/O is known.
        if tools.iter().any(|tool| {
            !matches!(
                tool.as_str(),
                "read_file" | "grep" | "glob" | "write_file" | "edit_file"
            )
        }) {
            return Ok(Self::DeviceWrite);
        }
        if tools.is_empty() {
            return Ok(Self::Scoped(Vec::new()));
        }
        let access = if tools
            .iter()
            .all(|tool| matches!(tool.as_str(), "read_file" | "grep" | "glob"))
        {
            WorkspaceAccess::Read
        } else {
            WorkspaceAccess::Write
        };
        Ok(Self::Scoped(vec![WorkspaceClaim::resolve(
            directory, access,
        )?]))
    }

    pub(crate) fn with_skill_reads(
        self,
        directories: impl IntoIterator<Item = PathBuf>,
    ) -> io::Result<Self> {
        let Self::Scoped(mut claims) = self else {
            return Ok(self);
        };
        if !claims.is_empty() {
            for directory in directories {
                claims.push(WorkspaceClaim::resolve(
                    &directory,
                    WorkspaceAccess::Read,
                )?);
            }
        }
        Ok(Self::Scoped(claims))
    }

    fn writes(&self) -> bool {
        match self {
            Self::DeviceWrite => true,
            Self::Scoped(claims) => claims
                .iter()
                .any(|claim| claim.access == WorkspaceAccess::Write),
        }
    }

    fn conflicts_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Scoped(ours), Self::Scoped(theirs)) => {
                ours.iter().any(|ours| {
                    theirs.iter().any(|theirs| ours.conflicts_with(theirs))
                })
            }
            _ => true,
        }
    }

    fn covers(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::DeviceWrite, _) => true,
            (Self::Scoped(ours), Self::Scoped(theirs)) => theirs
                .iter()
                .all(|theirs| ours.iter().any(|ours| ours.covers(theirs))),
            (Self::Scoped(_), Self::DeviceWrite) => false,
        }
    }
}

#[derive(Debug)]
struct Reservation {
    class: ExecutionClass,
    workspace: WorkspaceRequest,
}

#[derive(Debug)]
struct Pending {
    reservation: Arc<Reservation>,
    enqueued_at: Instant,
    foreground_bypasses: u8,
}

impl Pending {
    fn is_background(&self) -> bool {
        self.reservation.class == ExecutionClass::Background
    }

    fn has_priority(&self, now: Instant) -> bool {
        self.is_background()
            && (self.foreground_bypasses >= PRIORITY_MAX_BYPASSES
                || now >= self.enqueued_at + PRIORITY_MAX_WAIT)
    }
}

#[derive(Debug, Default)]
struct State {
    pending: VecDeque<Pending>,
    active: Vec<Arc<Reservation>>,
    closed: bool,
    device_quarantined: bool,
}

impl State {
    fn available(&self, request: &Reservation) -> bool {
        let background_capacity = request.class == ExecutionClass::Foreground
            || self
                .active
                .iter()
                .filter(|active| active.class == ExecutionClass::Background)
                .count()
                < MAX_BACKGROUND;
        background_capacity
            && self.active.iter().all(|active| {
                !active.workspace.conflicts_with(&request.workspace)
            })
    }

    fn ready_position(&self, now: Instant) -> Option<usize> {
        if self.closed
            || self.device_quarantined
            || self.active.len() >= MAX_ACTIVE
        {
            return None;
        }
        let priority = self
            .pending
            .iter()
            .enumerate()
            .find(|(_, pending)| pending.has_priority(now));
        if let Some((index, candidate)) = priority
            && self.available(&candidate.reservation)
        {
            return Some(index);
        }
        let foreground = self.pending.iter().position(|candidate| {
            let request = &candidate.reservation;
            request.class == ExecutionClass::Foreground
                && self.available(request)
                // Give an aged background writer its next feasible opportunity.
                // Disjoint candidates can still use the unoccupied capacity.
                && priority.is_none_or(|(_, reserved)| {
                    !reserved.reservation.workspace.conflicts_with(&request.workspace)
                })
        });
        foreground.or_else(|| {
            if priority.is_some() {
                None
            } else {
                self.pending.iter().position(|candidate| {
                    candidate.is_background()
                        && self.available(&candidate.reservation)
                })
            }
        })
    }

    fn next_priority_deadline(&self, now: Instant) -> Option<Instant> {
        let oldest = self
            .pending
            .iter()
            .find(|candidate| candidate.is_background())?;
        (!oldest.has_priority(now))
            .then_some(oldest.enqueued_at + PRIORITY_MAX_WAIT)
    }
}

#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    changed: watch::Sender<()>,
    marker: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct StartCoordinator {
    shared: Arc<Shared>,
}

impl StartCoordinator {
    pub(crate) fn new(root: &Path) -> Self {
        let marker = root.join(EXECUTION_MARKER);
        let device_quarantined = !matches!(fs::symlink_metadata(&marker),
            Err(error) if error.kind() == io::ErrorKind::NotFound);
        let (changed, _) = watch::channel(());
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    device_quarantined,
                    ..State::default()
                }),
                changed,
                marker: Some(marker),
            }),
        }
    }

    #[cfg(test)]
    fn in_memory() -> Self {
        let (changed, _) = watch::channel(());
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                changed,
                marker: None,
            }),
        }
    }

    /// Attempt an immediate start. Pending candidates retain their priority.
    pub(crate) fn try_acquire(
        &self,
        class: ExecutionClass,
        workspace: WorkspaceRequest,
    ) -> Result<Option<ExecutionPermit>, AdmissionError> {
        self.enqueue(class, workspace).take_if_ready(Instant::now())
    }

    /// Wait for the complete resource set without holding any execution slot.
    pub(crate) async fn acquire(
        &self,
        class: ExecutionClass,
        workspace: WorkspaceRequest,
        cancel: &CancelToken,
    ) -> Result<ExecutionPermit, AdmissionError> {
        self.enqueue(class, workspace).wait(cancel).await
    }

    fn enqueue(
        &self,
        class: ExecutionClass,
        workspace: WorkspaceRequest,
    ) -> Candidate {
        let reservation = Arc::new(Reservation { class, workspace });
        let changes = self.shared.changed.subscribe();
        self.shared
            .state
            .lock()
            .unwrap()
            .pending
            .push_back(Pending {
                reservation: Arc::clone(&reservation),
                enqueued_at: Instant::now(),
                foreground_bypasses: 0,
            });
        self.shared.changed.send_replace(());
        Candidate {
            coordinator: self.clone(),
            reservation,
            changes,
        }
    }

    /// Stop granting candidates while still letting running holders wind down.
    pub(crate) fn close(&self) {
        self.shared.state.lock().unwrap().closed = true;
        self.shared.changed.send_replace(());
    }

    /// Recovery calls this before admission when an unscoped shell writer may
    /// have survived the previous host. Only clear it after proving quiescence.
    pub(crate) fn quarantine_device(&self) -> io::Result<()> {
        let mut state = self.shared.state.lock().unwrap();
        state.device_quarantined = true;
        let result = if self
            .shared
            .marker
            .as_ref()
            .is_some_and(|path| path.exists())
        {
            Ok(())
        } else {
            self.shared.create_marker()
        };
        drop(state);
        self.shared.changed.send_replace(());
        result
    }

    pub(crate) fn is_quarantined(&self) -> bool {
        self.shared.state.lock().unwrap().device_quarantined
    }

    pub(crate) fn clear_device_quarantine(
        &self,
        confirmed_no_surviving_writers: bool,
    ) -> io::Result<()> {
        if !confirmed_no_surviving_writers {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "writer quiescence must be explicitly confirmed",
            ));
        }
        let mut state = self.shared.state.lock().unwrap();
        if state.active.iter().any(|active| active.workspace.writes())
            || state
                .pending
                .iter()
                .any(|pending| pending.reservation.workspace.writes())
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "execution writers are still active or waiting",
            ));
        }
        self.shared.remove_marker()?;
        state.device_quarantined = false;
        drop(state);
        self.shared.changed.send_replace(());
        Ok(())
    }
}

impl Shared {
    fn create_marker(&self) -> io::Result<()> {
        let Some(path) = &self.marker else {
            return Ok(());
        };
        let mut marker = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        marker.write_all(b"A Flowy workspace writer may still be alive. Confirm quiescence before clearing.\n")?;
        marker.sync_all()
    }

    fn remove_marker(&self) -> io::Result<()> {
        let Some(path) = &self.marker else {
            return Ok(());
        };
        match fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionError {
    Closed,
    Cancelled,
    Quarantined,
    Storage,
}

struct Candidate {
    coordinator: StartCoordinator,
    reservation: Arc<Reservation>,
    changes: watch::Receiver<()>,
}

impl Candidate {
    fn take_if_ready(
        &self,
        now: Instant,
    ) -> Result<Option<ExecutionPermit>, AdmissionError> {
        let shared = &self.coordinator.shared;
        let mut state = shared.state.lock().unwrap();
        if state.closed {
            return Err(AdmissionError::Closed);
        }
        if state.device_quarantined {
            return Err(AdmissionError::Quarantined);
        }
        let Some(index) = state.ready_position(now) else {
            return Ok(None);
        };
        if !Arc::ptr_eq(&state.pending[index].reservation, &self.reservation) {
            return Ok(None);
        }
        let request = Arc::clone(&state.pending[index].reservation);
        if request.workspace.writes()
            && !state.active.iter().any(|active| active.workspace.writes())
            && shared.create_marker().is_err()
        {
            state.device_quarantined = true;
            drop(state);
            shared.changed.send_replace(());
            return Err(AdmissionError::Storage);
        }
        state.pending.remove(index);
        if request.class == ExecutionClass::Foreground {
            for waiting in state
                .pending
                .iter_mut()
                .filter(|waiting| waiting.is_background())
            {
                waiting.foreground_bypasses =
                    waiting.foreground_bypasses.saturating_add(1);
            }
        }
        state.active.push(Arc::clone(&request));
        drop(state);
        shared.changed.send_replace(());
        Ok(Some(ExecutionPermit {
            coordinator: self.coordinator.clone(),
            reservation: request,
            // Admission itself has not launched any long-lived writer.
            completed: true,
        }))
    }

    async fn wait(
        mut self,
        cancel: &CancelToken,
    ) -> Result<ExecutionPermit, AdmissionError> {
        loop {
            if cancel.is_cancelled() {
                return Err(AdmissionError::Cancelled);
            }
            let now = Instant::now();
            if let Some(permit) = self.take_if_ready(now)? {
                return if cancel.is_cancelled() {
                    permit.complete();
                    Err(AdmissionError::Cancelled)
                } else {
                    Ok(permit)
                };
            }
            let deadline = {
                let state = self.coordinator.shared.state.lock().unwrap();
                if state.closed {
                    return Err(AdmissionError::Closed);
                }
                state.next_priority_deadline(now)
            };
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(AdmissionError::Cancelled),
                update = self.changes.changed() => {
                    if update.is_err() {
                        return Err(AdmissionError::Closed);
                    }
                }
                () = wait_for_priority(deadline) => {}
            }
        }
    }
}

async fn wait_for_priority(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
    } else {
        std::future::pending::<()>().await;
    }
}

impl Drop for Candidate {
    fn drop(&mut self) {
        let shared = &self.coordinator.shared;
        let mut state = shared.state.lock().unwrap();
        let previous = state.pending.len();
        state.pending.retain(|pending| {
            !Arc::ptr_eq(&pending.reservation, &self.reservation)
        });
        let removed = state.pending.len() != previous;
        drop(state);
        if removed {
            shared.changed.send_replace(());
        }
    }
}

/// Owns both resources and capacity. Move into the execution task, retaining
/// it through the real engine future and checking whether any dispatched tool
/// could have left a writer alive before releasing workspace admission.
#[derive(Debug)]
pub struct ExecutionPermit {
    coordinator: StartCoordinator,
    reservation: Arc<Reservation>,
    completed: bool,
}

impl ExecutionPermit {
    pub(crate) const fn started(mut self) -> Self {
        self.completed = false;
        self
    }

    /// Call only with a writer-quiescence proof (or before execution starts).
    /// The engine future finishing alone is insufficient after a native shell
    /// or unknown process-capable tool. Normal workers use settle instead.
    /// Dropping an unfinished writer retains the marker and quarantines.
    pub(crate) fn complete(mut self) {
        self.completed = true;
    }

    /// A normal shell exit can leave descendants alive. Without a stronger
    /// proof, retain the marker and quarantine rather than infer quiescence
    /// from the engine future having returned.
    pub(crate) fn settle(self, unproven_process_started: bool) {
        if unproven_process_started {
            tracing::warn!(target: "flowy.execution",
                "workspace writers quarantined after a tool without a quiescence proof");
            drop(self);
        } else {
            self.complete();
        }
    }

    pub(crate) fn covers(
        &self,
        class: ExecutionClass,
        request: &WorkspaceRequest,
    ) -> bool {
        self.reservation.class == class
            && self.reservation.workspace.covers(request)
    }
}

impl Drop for ExecutionPermit {
    fn drop(&mut self) {
        let shared = &self.coordinator.shared;
        let mut state = shared.state.lock().unwrap();
        state
            .active
            .retain(|active| !Arc::ptr_eq(active, &self.reservation));
        if self.reservation.workspace.writes()
            && (!self.completed
                || (!state.device_quarantined
                    && !state
                        .active
                        .iter()
                        .any(|active| active.workspace.writes())
                    && shared.remove_marker().is_err()))
        {
            state.device_quarantined = true;
        }
        drop(state);
        shared.changed.send_replace(());
    }
}

#[cfg(test)]
mod tests;
