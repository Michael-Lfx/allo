//! Background workflow execution.

use std::sync::Arc;

use nomi_types::{DetachedToolProgressGuard, ToolError};

use super::control::WorkflowRunControl;
use super::definition::WorkflowDefinition;
use super::definition::WorkflowPlan;
use super::executor::WorkflowExecutor;
use super::store::{WorkflowRunStatus, WorkflowRunStore};
use crate::backends::FlowyMediaServices;

/// Coordinates sync and async workflow runs.
pub struct WorkflowRunner {
    executor: Arc<WorkflowExecutor>,
    store: Arc<WorkflowRunStore>,
    control: WorkflowRunControl,
    async_execution: bool,
}

impl WorkflowRunner {
    pub fn new(services: FlowyMediaServices, store: Arc<WorkflowRunStore>) -> Self {
        let async_execution = services.media.workflows.async_execution;
        let max_retries = services.media.workflows.max_retries;
        let control = WorkflowRunControl::default();
        let executor = Arc::new(WorkflowExecutor::new(
            services,
            Arc::clone(&store),
            control.clone(),
            max_retries,
        ));
        Self {
            executor,
            store,
            control,
            async_execution,
        }
    }

    pub fn async_execution_enabled(&self) -> bool {
        self.async_execution
    }

    pub fn executor(&self) -> Arc<WorkflowExecutor> {
        Arc::clone(&self.executor)
    }

    pub fn store(&self) -> Arc<WorkflowRunStore> {
        Arc::clone(&self.store)
    }

    pub fn control(&self) -> &WorkflowRunControl {
        &self.control
    }

    /// Run synchronously (blocks until complete).
    pub async fn run_plan_sync(
        &self,
        plan: &WorkflowPlan,
    ) -> Result<super::store::WorkflowRunRecord, ToolError> {
        self.executor.run_plan(plan).await
    }

    /// Resume a failed workflow (e.g. long video after credit top-up).
    pub async fn resume_run_sync(
        &self,
        run_id: &str,
    ) -> Result<super::store::WorkflowRunRecord, ToolError> {
        self.executor.resume_run(run_id).await
    }

    /// Start async run; returns `run_id` immediately.
    pub fn spawn_plan(self: &Arc<Self>, plan: WorkflowPlan) -> Result<String, ToolError> {
        let def = WorkflowDefinition {
            id: plan.workflow_id.clone(),
            version: plan.template_version,
            description: String::new(),
            inputs: plan.inputs.clone(),
            steps: plan.steps.clone(),
        };
        let mut record = self.store.create_run(&def.id, def.inputs.clone());
        record.status = WorkflowRunStatus::Running;
        self.store.save(&record);
        let run_id = record.run_id.clone();
        let spawn_id = run_id.clone();

        let runner = Arc::clone(self);
        let def = def.clone();
        let detached = DetachedToolProgressGuard::attach(&run_id);
        let handle = tokio::spawn(async move {
            let _detached = detached;
            if let Err(err) = runner
                .executor
                .run_definition_existing(&spawn_id, &def)
                .await
            {
                if err.to_string().contains("cancelled") {
                    tracing::info!(run_id = %spawn_id, "async workflow run cancelled");
                } else {
                    tracing::error!(run_id = %spawn_id, error = %err, "async workflow run failed");
                }
            }
            runner.control.unregister(&spawn_id);
        });
        self.control.register(&run_id, handle.abort_handle());
        Ok(run_id)
    }

    /// Resume async run; returns same `run_id`.
    pub fn spawn_resume(self: &Arc<Self>, run_id: &str) -> Result<String, ToolError> {
        let Some(record) = self.store.get(run_id) else {
            return Err(ToolError::ExecutionFailed(format!(
                "workflow run not found: {run_id}"
            )));
        };
        if record.status == WorkflowRunStatus::Running
            && !crate::long_video_active::record_is_resumable(
                &record,
                None,
                self.control.contains(run_id),
            )
        {
            return Ok(run_id.to_string());
        }
        if record.status == WorkflowRunStatus::Succeeded {
            return Ok(run_id.to_string());
        }

        let spawn_id = run_id.to_string();
        let register_id = spawn_id.clone();
        let runner = Arc::clone(self);
        let detached = DetachedToolProgressGuard::attach(&spawn_id);
        let handle = tokio::spawn(async move {
            let _detached = detached;
            if let Err(err) = runner.executor.resume_run(&spawn_id).await {
                if err.to_string().contains("cancelled") {
                    tracing::info!(run_id = %spawn_id, "async workflow resume cancelled");
                } else {
                    tracing::error!(run_id = %spawn_id, error = %err, "async workflow resume failed");
                }
            }
            runner.control.unregister(&spawn_id);
        });
        self.control.register(&register_id, handle.abort_handle());
        Ok(register_id)
    }

    /// Cancel a running workflow by `run_id`.
    pub async fn cancel_run(&self, run_id: &str) -> Result<(), ToolError> {
        let video_id = self.control.cancel(run_id);
        if let Some(local_id) = video_id
            && let Ok(token) = self.executor.services.require_token().await
        {
            let session = &self.executor.services.session;
            if let Err(err) = self
                .executor
                .services
                .api
                .cancel_video_task(session, local_id)
                .await
            {
                tracing::warn!(
                    run_id = %run_id,
                    local_id,
                    error = %err,
                    "server video cancel failed (local task aborted)"
                );
            }
            let _ = token;
        }

        if let Some(mut record) = self.store.get(run_id) {
            if record.status == WorkflowRunStatus::Running {
                record.status = WorkflowRunStatus::Cancelled;
                record.error = Some("cancelled by user".into());
                record.current_step = None;
                self.store.save(&record);
            }
            return Ok(());
        }

        if self.control.contains(run_id) {
            return Ok(());
        }

        Err(ToolError::ExecutionFailed(format!(
            "workflow run not found or already finished: {run_id}"
        )))
    }
}
