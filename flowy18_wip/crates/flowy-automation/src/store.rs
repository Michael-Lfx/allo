//! SQLite persistence owned by one dedicated thread. No connection or raw SQL
//! crosses the repository boundary; callers await messages without doing I/O.

mod audit;
mod records;
mod repository;
mod schema;
#[cfg(test)]
mod tests;
mod validation;

pub use audit::{
    AuditFinding, AuditReport, InventoryCompleteness, SideRecord,
    SideStoreInventory,
};
pub use records::*;

use crate::{AutomationId, DeletionPlanId, PrincipalId, RequestKey, RunId};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use tokio::sync::oneshot;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("automation SQLite operation failed")]
    Sqlite(#[from] rusqlite::Error),
    #[error("automation storage I/O failed")]
    Io(#[from] std::io::Error),
    #[error("automation stored JSON is invalid")]
    Json(#[from] serde_json::Error),
    #[error("automation dataset is incompatible: {0}")]
    Incompatible(&'static str),
    #[error("automation dataset is corrupt: {0}")]
    Corrupt(&'static str),
    #[error("invalid automation storage request: {0}")]
    Invalid(&'static str),
    #[error("automation database worker is unavailable")]
    Unavailable,
    #[error("automation storage revision conflict")]
    Conflict,
    #[error("automation logical reference failed: {0}")]
    Reference(&'static str),
}

type Job = Box<dyn FnOnce(&mut Connection) + Send>;
enum Message {
    Work(Job),
    Shutdown(oneshot::Sender<()>),
}

/// The worker owns the supplied store lease until its connection is closed.
/// Production supplies the `AppHost` lease; no second instance lock is acquired.
#[derive(Clone)]
pub struct AutomationStore {
    sender: mpsc::Sender<Message>,
}

impl std::fmt::Debug for AutomationStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutomationStore").finish_non_exhaustive()
    }
}

impl AutomationStore {
    pub async fn open(
        root: PathBuf,
        owner: Arc<dyn Send + Sync>,
    ) -> Result<Self, StoreError> {
        let (sender, receiver) = mpsc::channel();
        let (ready, initialized) = oneshot::channel();
        std::thread::Builder::new()
            .name("flowy-automation-db".into())
            .spawn(move || {
                let storage_owner = owner;
                let path =
                    root.join(crate::schema_contract::DATABASE_RELATIVE_PATH);
                let mut connection = match schema::open(&path) {
                    Ok(connection) => connection,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                if ready.send(Ok(())).is_err() {
                    return;
                }
                while let Ok(message) = receiver.recv() {
                    match message {
                        Message::Work(job) => job(&mut connection),
                        Message::Shutdown(done) => {
                            drop(connection);
                            drop(storage_owner);
                            let _ = done.send(());
                            return;
                        }
                    }
                }
            })?;
        initialized.await.map_err(|_| StoreError::Unavailable)??;
        Ok(Self { sender })
    }

    async fn call<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, StoreError>
        + Send
        + 'static,
    ) -> Result<T, StoreError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Message::Work(Box::new(move |connection| {
                let _ = reply.send(operation(connection));
            })))
            .map_err(|_| StoreError::Unavailable)?;
        response.await.map_err(|_| StoreError::Unavailable)?
    }

    /// All mutations and their owner/parent checks commit or roll back together.
    pub async fn commit(
        &self,
        principal: PrincipalId,
        mutations: Vec<StoreMutation>,
    ) -> Result<(), StoreError> {
        self.call(move |connection| {
            repository::commit(connection, &principal, mutations)
        })
        .await
    }

    pub async fn automation(
        &self,
        principal: PrincipalId,
        id: AutomationId,
    ) -> Result<Option<AutomationRecord>, StoreError> {
        self.call(move |connection| {
            repository::automation(connection, &principal, &id)
        })
        .await
    }
    pub async fn revisions(
        &self,
        principal: PrincipalId,
        id: AutomationId,
    ) -> Result<Vec<RevisionRecord>, StoreError> {
        self.call(move |connection| {
            repository::revisions(connection, &principal, &id)
        })
        .await
    }
    pub async fn run(
        &self,
        principal: PrincipalId,
        id: RunId,
    ) -> Result<Option<RunRecord>, StoreError> {
        self.call(move |connection| {
            repository::run(connection, &principal, &id)
        })
        .await
    }
    pub async fn attempts(
        &self,
        principal: PrincipalId,
        id: RunId,
    ) -> Result<Vec<AttemptRecord>, StoreError> {
        self.call(move |connection| {
            repository::attempts(connection, &principal, &id)
        })
        .await
    }
    pub async fn receipt(
        &self,
        principal: PrincipalId,
        key: RequestKey,
    ) -> Result<Option<ReceiptRecord>, StoreError> {
        self.call(move |connection| {
            repository::receipt(connection, &principal, &key)
        })
        .await
    }
    pub async fn outbox(
        &self,
        principal: PrincipalId,
        id: RunId,
    ) -> Result<Vec<OutboxRecord>, StoreError> {
        self.call(move |connection| {
            repository::outbox(connection, &principal, &id)
        })
        .await
    }
    pub async fn deletion_plan(
        &self,
        principal: PrincipalId,
        id: DeletionPlanId,
    ) -> Result<Option<DeletionPlanRecord>, StoreError> {
        self.call(move |connection| {
            repository::deletion_plan(connection, &principal, &id)
        })
        .await
    }
    pub async fn watermark(
        &self,
        principal: PrincipalId,
        id: AutomationId,
    ) -> Result<Option<WatermarkRecord>, StoreError> {
        self.call(move |connection| {
            repository::watermark(connection, &principal, &id)
        })
        .await
    }
    pub async fn audit(
        &self,
        inventory: SideStoreInventory,
    ) -> Result<AuditReport, StoreError> {
        self.call(move |connection| audit::audit(connection, &inventory))
            .await
    }

    /// Explicitly closes the worker for every clone after earlier queued jobs.
    pub async fn shutdown(&self) -> Result<(), StoreError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Message::Shutdown(reply))
            .map_err(|_| StoreError::Unavailable)?;
        response.await.map_err(|_| StoreError::Unavailable)
    }
}
