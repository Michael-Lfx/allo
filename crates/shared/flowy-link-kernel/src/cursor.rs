/// How a client catches up after a socket drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayPlan {
    /// Apply events with `seq` greater than `last_applied`.
    Continue { from: u64 },
    /// The gap is larger than the server buffer. Reload the domain snapshot.
    Snapshot,
}

/// `buffer` is how many sequence numbers the server still retains.
pub fn plan_replay(last_applied: u64, newest: u64, buffer: u64) -> ReplayPlan {
    if newest <= last_applied {
        return ReplayPlan::Continue { from: last_applied };
    }
    let gap = newest - last_applied;
    if gap > buffer {
        ReplayPlan::Snapshot
    } else {
        ReplayPlan::Continue { from: last_applied }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_gap_replays() {
        assert_eq!(plan_replay(10, 12, 100), ReplayPlan::Continue { from: 10 });
    }

    #[test]
    fn large_gap_snapshots() {
        assert_eq!(plan_replay(1, 500, 100), ReplayPlan::Snapshot);
    }
}
