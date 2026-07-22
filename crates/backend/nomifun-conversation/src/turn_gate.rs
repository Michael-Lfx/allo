//! Pure turn phase × command matrix shared by admission, cancel, and finish.

/// Coarse turn phase. Not persisted — derived from active turn + cleanup fences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnPhase {
    Idle,
    Running,
    Finishing,
    Cancelling,
}

/// Commands the turn gate accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnCommand {
    Admit,
    Cancel,
    Finish,
}

/// Pure gate: whether `command` is allowed in `phase` without side effects.
///
/// ```text
/// Phase \ Command | Admit | Cancel | Finish
/// ----------------|-------|--------|-------
/// Idle            | ok    | no-op  | reject
/// Running         | busy  | ok     | ok
/// Finishing       | reject| ok     | ok
/// Cancelling      | reject| ok     | ok
/// ```
pub(crate) fn turn_command_allowed(phase: TurnPhase, command: TurnCommand) -> bool {
    match (phase, command) {
        (TurnPhase::Idle, TurnCommand::Admit) => true,
        (TurnPhase::Idle, TurnCommand::Cancel) => true,
        (TurnPhase::Idle, TurnCommand::Finish) => false,
        (TurnPhase::Running, TurnCommand::Admit) => false,
        (TurnPhase::Running, TurnCommand::Cancel) => true,
        (TurnPhase::Running, TurnCommand::Finish) => true,
        (TurnPhase::Finishing, TurnCommand::Admit) => false,
        (TurnPhase::Finishing, TurnCommand::Cancel) => true,
        (TurnPhase::Finishing, TurnCommand::Finish) => true,
        (TurnPhase::Cancelling, TurnCommand::Admit) => false,
        (TurnPhase::Cancelling, TurnCommand::Cancel) => true,
        (TurnPhase::Cancelling, TurnCommand::Finish) => true,
    }
}

pub(crate) fn derive_turn_phase(
    has_active_turn: bool,
    stop_in_progress: bool,
    completion_in_progress: bool,
) -> TurnPhase {
    if stop_in_progress {
        TurnPhase::Cancelling
    } else if completion_in_progress {
        TurnPhase::Finishing
    } else if has_active_turn {
        TurnPhase::Running
    } else {
        TurnPhase::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_matches_documented_table() {
        let cases = [
            (TurnPhase::Idle, TurnCommand::Admit, true),
            (TurnPhase::Idle, TurnCommand::Cancel, true),
            (TurnPhase::Idle, TurnCommand::Finish, false),
            (TurnPhase::Running, TurnCommand::Admit, false),
            (TurnPhase::Running, TurnCommand::Cancel, true),
            (TurnPhase::Running, TurnCommand::Finish, true),
            (TurnPhase::Finishing, TurnCommand::Admit, false),
            (TurnPhase::Finishing, TurnCommand::Cancel, true),
            (TurnPhase::Finishing, TurnCommand::Finish, true),
            (TurnPhase::Cancelling, TurnCommand::Admit, false),
            (TurnPhase::Cancelling, TurnCommand::Cancel, true),
            (TurnPhase::Cancelling, TurnCommand::Finish, true),
        ];
        for (phase, command, allowed) in cases {
            assert_eq!(
                turn_command_allowed(phase, command),
                allowed,
                "{phase:?} + {command:?}"
            );
        }
    }

    #[test]
    fn derive_prefers_cancel_over_finish_over_running() {
        assert_eq!(
            derive_turn_phase(true, true, true),
            TurnPhase::Cancelling
        );
        assert_eq!(
            derive_turn_phase(true, false, true),
            TurnPhase::Finishing
        );
        assert_eq!(derive_turn_phase(true, false, false), TurnPhase::Running);
        assert_eq!(derive_turn_phase(false, false, false), TurnPhase::Idle);
    }
}
