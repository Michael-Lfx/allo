//! Independent-process provision probe for the production model verification chain.
//!
//! Usage: `cargo run -p voice-companion --example provision_stack_probe -- <stack-mb> [models-dir]`
//! It never downloads, opens audio devices, or creates a session.

use std::sync::Arc;

use nomifun_voice_models::ModelManager;
use voice_companion::config::Config;
use voice_companion::provision::check_and_auto_download;

fn main() {
    let stack_mb = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1".into())
        .parse::<usize>()
        .expect("stack size in megabytes");
    let models_dir = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "D:/tmp/flowy-dev/models".into());
    let manager = Arc::new(ModelManager::new(&models_dir));
    let mut config = Config::default();
    config.models.auto_download = false;
    config.models.models_dir = models_dir.into();

    let worker = std::thread::Builder::new()
        .name(format!("provision-{stack_mb}mb"))
        .stack_size(stack_mb * 1024 * 1024)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let downloaded = check_and_auto_download(&config, &manager).await;
                println!("provision_finished={downloaded}");
            });
        })
        .unwrap();
    worker.join().expect("provision thread must not overflow its stack");
}
