//! AI Mate Voice Companion Daemon Entry Point

use ai_mate::config::Config;

#[tokio::main]
async fn main() {
    let _config = Config::load().unwrap_or_default();
    println!("Flowy AI Mate Voice Companion Daemon");
}
