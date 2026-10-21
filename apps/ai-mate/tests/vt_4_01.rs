//! Vertical Slice Test VT-4-01:
//! 三端宿主服务安装描述符完整性、systemd WatchdogSec=30s、非特权用户隔离与待机资源约束断言。
//!
//! Baseline:
//! - `interaction.zh.md` §3.1, §5.1 (TC-AM-012)
//! - `execution/wave-4-hardening.zh.md` §2 (T4-01, T4-02), §3 (VT-4-01)

use std::path::Path;

use ai_mate::watchdog::{get_recommended_heartbeat_interval, WATCHDOG_TIMEOUT_SECS};

#[test]
fn test_vt_4_01_linux_systemd_unit_contract() {
    let service_path = Path::new("packaging/linux/ai-mate.service");
    let content = std::fs::read_to_string(service_path)
        .expect("packaging/linux/ai-mate.service must exist");

    // 1. Type=notify
    assert!(
        content.contains("Type=notify"),
        "systemd service must declare Type=notify for sd_notify integration"
    );

    // 2. Dedicated non-privileged user isolation: aimate
    assert!(
        content.contains("User=aimate") && content.contains("Group=aimate"),
        "systemd service must run under isolated non-privileged user 'aimate'"
    );

    // 3. WatchdogSec=30s
    assert!(
        content.contains("WatchdogSec=30s"),
        "systemd service must configure WatchdogSec=30s"
    );

    // 4. Working directory and restart policy
    assert!(
        content.contains("WorkingDirectory=/var/lib/ai-mate"),
        "WorkingDirectory must point to /var/lib/ai-mate"
    );
    assert!(
        content.contains("Restart=always"),
        "Restart must be set to always"
    );
}

#[test]
fn test_vt_4_01_macos_launchd_descriptor_contract() {
    let plist_path = Path::new("packaging/macos/com.flowy.aimate.plist");
    let content = std::fs::read_to_string(plist_path)
        .expect("packaging/macos/com.flowy.aimate.plist must exist");

    assert!(
        content.contains("com.flowy.aimate"),
        "plist must declare label com.flowy.aimate"
    );
    assert!(
        content.contains("<key>KeepAlive</key>") && content.contains("<true/>"),
        "plist must configure KeepAlive = true"
    );
    assert!(
        content.contains("<key>RunAtLoad</key>") && content.contains("<true/>"),
        "plist must configure RunAtLoad = true"
    );
}

#[test]
fn test_vt_4_01_windows_service_descriptor_contract() {
    let xml_path = Path::new("packaging/windows/ai-mate-service.xml");
    let content = std::fs::read_to_string(xml_path)
        .expect("packaging/windows/ai-mate-service.xml must exist");

    assert!(
        content.contains("<id>FlowyAiMate</id>"),
        "XML must declare service id FlowyAiMate"
    );
    assert!(
        content.contains("<startmode>Automatic</startmode>"),
        "XML must configure startmode Automatic"
    );
}

#[test]
fn test_vt_4_01_watchdog_watermark_interval() {
    assert_eq!(
        WATCHDOG_TIMEOUT_SECS, 30,
        "WatchdogSec specification is 30s"
    );
    let interval = get_recommended_heartbeat_interval();
    assert_eq!(
        interval.as_secs(),
        15,
        "Heartbeat interval must be <= 1/2 of WatchdogSec (15s safe watermark)"
    );
}
