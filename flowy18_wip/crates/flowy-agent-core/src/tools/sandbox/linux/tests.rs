use super::super::{SandboxAvailability, SandboxBackend, SandboxPolicy, unix};
use super::availability;
use rpi_tools::{ExecutionEnv, ExecutionErrorCode, ShellExecOptions};
use std::collections::HashMap;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const READY_COMMAND: &str = "sleep 60 & printf ready > ready; wait";
const REAP_WORKSPACE: &str = "FLOWY_TEST_BWRAP_REAP_WORKSPACE";
const REAP_TEST: &str =
    "tools::sandbox::linux::tests::linux_bwrap_reaps_on_host_exit";

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("build-test")
            .join(format!(
                "flowy-linux-sandbox-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(root.join("work")).unwrap();
        Self(root)
    }

    fn workspace(&self) -> PathBuf {
        self.0.join("work")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn active_env(
    workspace: &Path,
    policy: SandboxPolicy,
) -> Arc<dyn ExecutionEnv> {
    assert_eq!(
        availability(),
        &SandboxAvailability::Enforced {
            backend: SandboxBackend::LinuxBubblewrap,
        },
        "integration tests require a working nonprivileged Bubblewrap"
    );
    unix::execution_env(workspace, policy, None)
}

fn collect_descendants(pid: u32, descendants: &mut Vec<u32>) {
    let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) else {
        return;
    };
    for task in tasks.flatten() {
        let Ok(children) = fs::read_to_string(task.path().join("children"))
        else {
            continue;
        };
        for child in children
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
        {
            descendants.push(child);
            collect_descendants(child, descendants);
        }
    }
}

fn wait_for_reaped(descendants: &[u32]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while descendants
        .iter()
        .any(|pid| Path::new(&format!("/proc/{pid}")).exists())
    {
        assert!(
            Instant::now() < deadline,
            "processes still present: {descendants:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[tokio::test]
#[ignore = "requires nonprivileged Bubblewrap with mount, PID, and network namespaces"]
async fn linux_bwrap_masks_credentials_and_blocks_network_when_requested() {
    let scratch = Scratch::new();
    let cwd = scratch.workspace();
    let auth = scratch.0.join("auth");
    fs::create_dir(&auth).unwrap();
    fs::write(auth.join("fixture-token"), "synthetic").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let _connection = TcpStream::connect(addr).unwrap();
    let env = active_env(
        &cwd,
        SandboxPolicy::for_workspace(cwd.clone())
            .with_protected_path(auth)
            .with_network(false),
    );
    let command = format!(
        r#"set -e
test -z "$FLOWY_LLM_API_KEY"
test -d ../auth
test ! -e ../auth/fixture-token
if (: >/dev/tcp/127.0.0.1/{}) 2>/dev/null; then exit 12; fi
printf isolated"#,
        addr.port()
    );
    let result = env
        .exec(
            &command,
            ShellExecOptions {
                env: Some(HashMap::from([(
                    "FLOWY_LLM_API_KEY".to_owned(),
                    "synthetic-environment-value".to_owned(),
                )])),
                timeout: Some(5.0),
                ..ShellExecOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "isolated");
}

#[tokio::test]
#[ignore = "requires nonprivileged Bubblewrap with mount and PID namespaces"]
async fn linux_bwrap_prepares_private_host_auth_before_launch() {
    assert!(availability().is_enforced());
    let scratch = Scratch::new();
    let cwd = scratch.workspace();
    let auth = scratch.0.join("auth");
    assert!(!auth.exists());

    let env = unix::execution_env(
        &cwd,
        SandboxPolicy::for_workspace(cwd.clone()),
        Some(auth.clone()),
    );
    let shell = env.exec(
        r"set -e
          stat -c '%d:%i' ../auth
          : > ready
          while [ ! -f saved ]; do sleep 0.02; done
          test ! -e ../auth/synthetic-credential",
        ShellExecOptions {
            timeout: Some(8.0),
            ..ShellExecOptions::default()
        },
    );
    let save_session = async {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !cwd.join("ready").is_file() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("sandbox started with a masked auth directory");
        fs::write(auth.join("synthetic-credential"), "fixture").unwrap();
        fs::write(cwd.join("saved"), "").unwrap();
    };
    let (result, ()) = tokio::join!(shell, save_session);
    let result = result.unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    let metadata = auth.metadata().unwrap();
    let host_identity = format!("{}:{}", metadata.dev(), metadata.ino());
    assert_ne!(result.stdout.trim(), "");
    assert_ne!(result.stdout.trim(), host_identity);
    assert_eq!(metadata.permissions().mode() & 0o077, 0);
    assert!(auth.join("synthetic-credential").is_file());
}

#[test]
fn host_auth_preparation_rejects_a_file() {
    let scratch = Scratch::new();
    let auth = scratch.0.join("auth");
    fs::write(&auth, "fixture").unwrap();
    let error = super::prepare_host_auth(&auth).unwrap_err();
    assert_eq!(error.code, ExecutionErrorCode::SpawnError);
    assert_eq!(fs::read_to_string(auth).unwrap(), "fixture");
}

#[tokio::test]
#[ignore = "requires nonprivileged Bubblewrap with mount and PID namespaces"]
async fn linux_bwrap_forwards_safe_proxy_environment() {
    let scratch = Scratch::new();
    let cwd = scratch.workspace();
    let env = active_env(&cwd, SandboxPolicy::for_workspace(cwd.clone()));
    let result = env
        .exec(
            r#"test "$https_proxy" = 'http://127.0.0.1:18080' &&
               test "$NO_PROXY" = 'localhost,.internal' &&
               test -z "$ALL_PROXY" && printf proxy-ready"#,
            ShellExecOptions {
                env: Some(HashMap::from([
                    (
                        "https_proxy".to_owned(),
                        "http://127.0.0.1:18080".to_owned(),
                    ),
                    ("NO_PROXY".to_owned(), "localhost,.internal".to_owned()),
                    (
                        "ALL_PROXY".to_owned(),
                        "http://synthetic:secret@127.0.0.1:18080".to_owned(),
                    ),
                ])),
                inherit_env: false,
                timeout: Some(5.0),
                ..ShellExecOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "proxy-ready");
}

#[tokio::test]
#[ignore = "requires nonprivileged Bubblewrap with mount and PID namespaces"]
async fn linux_bwrap_preserves_resolver_for_allowed_network() {
    let scratch = Scratch::new();
    let cwd = scratch.workspace();
    let resolver = fs::read_to_string("/etc/resolv.conf").unwrap();
    let env = active_env(&cwd, SandboxPolicy::for_workspace(cwd.clone()));
    let result = env
        .exec(
            "/bin/cat /etc/resolv.conf",
            ShellExecOptions {
                timeout: Some(5.0),
                ..ShellExecOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, resolver);
}

#[tokio::test]
#[ignore = "requires nonprivileged Bubblewrap with mount and PID namespaces"]
async fn linux_bwrap_reaps_on_cancel_and_timeout() {
    for cancel_requested in [true, false] {
        let scratch = Scratch::new();
        let cwd = scratch.workspace();
        let env = active_env(&cwd, SandboxPolicy::for_workspace(cwd.clone()));
        let cancel = CancellationToken::new();
        let running = env.exec(
            READY_COMMAND,
            ShellExecOptions {
                cancel: Some(&cancel),
                timeout: Some(if cancel_requested { 10.0 } else { 4.0 }),
                ..ShellExecOptions::default()
            },
        );
        let observe = async {
            tokio::time::timeout(Duration::from_secs(3), async {
                while !cwd.join("ready").is_file() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("sandbox reached the running child");
            let mut descendants = Vec::new();
            collect_descendants(std::process::id(), &mut descendants);
            assert!(descendants.len() >= 2);
            if cancel_requested {
                cancel.cancel();
            }
            descendants
        };
        let (result, descendants) = tokio::join!(running, observe);
        let expected = if cancel_requested {
            ExecutionErrorCode::Aborted
        } else {
            ExecutionErrorCode::Timeout
        };
        assert_eq!(result.unwrap_err().code, expected);
        wait_for_reaped(&descendants);
    }
}

#[test]
#[ignore = "requires nonprivileged Bubblewrap with mount and PID namespaces"]
fn linux_bwrap_reaps_on_host_exit() {
    if let Some(workspace) = std::env::var_os(REAP_WORKSPACE) {
        let cwd = PathBuf::from(workspace);
        let env = active_env(&cwd, SandboxPolicy::for_workspace(cwd.clone()));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(env.exec(
            READY_COMMAND,
            ShellExecOptions {
                timeout: Some(12.0),
                ..ShellExecOptions::default()
            },
        ));
        panic!("test host unexpectedly completed its command: {result:?}");
    }

    let scratch = Scratch::new();
    let cwd = scratch.workspace();
    let mut host = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", REAP_TEST])
        .env(REAP_WORKSPACE, &cwd)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while !cwd.join("ready").is_file() && Instant::now() < deadline {
        assert!(
            host.try_wait().unwrap().is_none(),
            "test host exited before readiness"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut descendants = Vec::new();
    collect_descendants(host.id(), &mut descendants);
    host.kill().unwrap();
    host.wait().unwrap();
    assert!(
        cwd.join("ready").is_file(),
        "sandbox child never became ready"
    );
    assert!(descendants.len() >= 2);
    wait_for_reaped(&descendants);
}
