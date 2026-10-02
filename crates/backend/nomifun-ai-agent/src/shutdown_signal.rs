use tokio::sync::watch;

/// Resolves once the channel carries `true`.
///
/// A dropped sender that never signalled does not count as a shutdown request:
/// `watch::Receiver::changed` then fails immediately on every call, so a loop
/// that merely ignores the error would spin a CPU core. In that case this
/// future stays pending for good.
pub(crate) async fn shutdown_requested(shutdown: &mut watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow_and_update() {
            return;
        }
        if shutdown.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use super::*;

    #[tokio::test]
    async fn stays_pending_while_the_signal_is_false() {
        let (_tx, mut rx) = watch::channel(false);
        assert!(shutdown_requested(&mut rx).now_or_never().is_none());
    }

    #[tokio::test]
    async fn resolves_when_true_is_sent() {
        let (tx, mut rx) = watch::channel(false);
        let waiter = tokio::spawn(async move { shutdown_requested(&mut rx).await });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());

        tx.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("waiter should resolve after the signal")
            .unwrap();
    }

    #[tokio::test]
    async fn resolves_when_true_was_sent_before_the_first_poll() {
        let (tx, mut rx) = watch::channel(false);
        tx.send(true).unwrap();
        assert!(shutdown_requested(&mut rx).now_or_never().is_some());
    }

    #[tokio::test]
    async fn resolves_when_the_sender_drops_right_after_signalling() {
        let (tx, mut rx) = watch::channel(false);
        tx.send(true).unwrap();
        drop(tx);
        assert!(shutdown_requested(&mut rx).now_or_never().is_some());
    }

    #[tokio::test]
    async fn dropped_sender_without_signal_is_not_a_shutdown() {
        let (tx, mut rx) = watch::channel(false);
        drop(tx);
        assert!(shutdown_requested(&mut rx).now_or_never().is_none());
    }

    #[tokio::test]
    async fn dropped_sender_does_not_starve_a_timer_in_select() {
        let (tx, mut rx) = watch::channel(false);
        drop(tx);

        let mut interval = tokio::time::interval(std::time::Duration::from_millis(5));
        let mut ticks = 0;
        while ticks < 3 {
            tokio::select! {
                _ = interval.tick() => ticks += 1,
                () = shutdown_requested(&mut rx) => panic!("a dropped sender is not a shutdown"),
            }
        }
    }
}
