//! Host authorization for one run's external requests.

use std::error::Error;
use std::fmt;

use crate::{ExecutionMetadata, ModelRoute, ToolCallArgs, ToolRunContext};

/// Stable denial categories without credentials, principal IDs, or request data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionDenied {
    AccountUnavailable,
    OwnerMismatch,
    LeaseExpired,
    CredentialUnavailable,
    ModelUnavailable,
    ToolDenied,
}

impl fmt::Display for ExecutionDenied {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AccountUnavailable => "execution account is unavailable",
            Self::OwnerMismatch => "execution does not belong to this account",
            Self::LeaseExpired => {
                "execution authorization is no longer current"
            }
            Self::CredentialUnavailable => {
                "execution credential is unavailable"
            }
            Self::ModelUnavailable => "execution model is unavailable",
            Self::ToolDenied => "execution is not authorized for this tool",
        })
    }
}

impl Error for ExecutionDenied {}

/// Resolve access against current host state. A successful decision belongs
/// to this dispatch and must never be cached for a later turn or retry.
pub trait ExecutionGate: Send + Sync {
    fn model_credential(
        &self,
        execution: &ExecutionMetadata,
        route: &ModelRoute,
    ) -> Result<String, ExecutionDenied>;

    fn authorize_tool(
        &self,
        context: &ToolRunContext,
        args: &ToolCallArgs,
    ) -> Result<(), ExecutionDenied>;
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    pub struct RevocableGate(pub AtomicBool);

    impl RevocableGate {
        fn check(&self) -> Result<(), ExecutionDenied> {
            if self.0.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(ExecutionDenied::LeaseExpired)
            }
        }
    }

    impl ExecutionGate for RevocableGate {
        fn model_credential(
            &self,
            _execution: &ExecutionMetadata,
            _route: &ModelRoute,
        ) -> Result<String, ExecutionDenied> {
            self.check().map(|()| "fixture".to_owned())
        }

        fn authorize_tool(
            &self,
            _context: &ToolRunContext,
            _args: &ToolCallArgs,
        ) -> Result<(), ExecutionDenied> {
            self.check()
        }
    }
}
