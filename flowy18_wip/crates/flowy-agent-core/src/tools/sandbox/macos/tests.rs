use super::super::{SandboxAvailability, SandboxBackend, unix};
use super::{availability, shell_override};
use crate::tools::SandboxPolicy;
use rpi_tools::{ExecutionErrorCode, ShellExecOptions};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[test]
fn seatbelt_profile_file_is_private_and_removed() {
    let (_, profile) = shell_override(&SandboxPolicy::for_workspace(
        Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf(),
    ))
    .unwrap();
    let path = profile.path().to_path_buf();
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .starts_with("(version 1)")
    );
    drop(profile);
    assert!(!path.exists());
}

#[test]
fn seatbelt_rejects_protected_paths_it_cannot_encode() {
    let policy = SandboxPolicy::for_workspace(
        Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf(),
    )
    .with_protected_path(std::path::PathBuf::from("/cannot\nencode"));
    let error = shell_override(&policy).err().unwrap();
    assert_eq!(error.code, ExecutionErrorCode::SpawnError);
}

#[tokio::test]
#[ignore = "requires macOS Seatbelt and a working sandbox-exec"]
async fn macos_seatbelt_filters_environment_and_blocks_network_when_requested()
{
    assert_eq!(
        availability(),
        &SandboxAvailability::Enforced {
            backend: SandboxBackend::MacOsSeatbelt,
        },
    );
    let cwd = Path::new(env!("CARGO_MANIFEST_DIR"));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let connect = format!("/usr/bin/nc -z -w 1 127.0.0.1 {}", addr.port());
    let policy = SandboxPolicy::for_workspace(cwd.to_path_buf());
    let allowed = unix::execution_env(cwd, policy.clone(), None)
        .exec(
            &connect,
            ShellExecOptions {
                timeout: Some(5.0),
                ..ShellExecOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(allowed.exit_code, 0, "{}", allowed.stderr);
    let env = unix::execution_env(cwd, policy.with_network(false), None);
    let command = format!(
        r#"set -e; test -z "$FLOWY_LLM_API_KEY"; test -z "$http_proxy"; if {connect} 2>/dev/null; then exit 12; fi; printf isolated"#
    );
    let result = env
        .exec(
            &command,
            ShellExecOptions {
                env: Some(HashMap::from([
                    (
                        "FLOWY_LLM_API_KEY".to_owned(),
                        "synthetic-environment-value".to_owned(),
                    ),
                    ("http_proxy".to_owned(), format!("http://{addr}")),
                ])),
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
#[ignore = "requires macOS Seatbelt, sandbox-exec, and system curl"]
async fn macos_seatbelt_requests_through_safe_proxy() {
    assert!(availability().is_enforced());
    let cwd = Path::new(env!("CARGO_MANIFEST_DIR"));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    let env = unix::execution_env(
        cwd,
        SandboxPolicy::for_workspace(cwd.to_path_buf()),
        None,
    );
    let request = env.exec(
        "/usr/bin/curl -q -fsS --max-time 4 http://proxy-fixture.invalid/check",
        ShellExecOptions {
            env: Some(HashMap::from([("http_proxy".to_owned(), proxy)])),
            inherit_env: false,
            timeout: Some(6.0),
            ..ShellExecOptions::default()
        },
    );
    let serve = async {
        let mut socket = tokio::time::timeout(Duration::from_secs(6), async {
            loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(error) => panic!("test proxy accept failed: {error}"),
                }
            }
        })
        .await
        .expect("curl reached the local proxy");
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut reader = BufReader::new(&socket);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(
            line.trim_end(),
            "GET http://proxy-fixture.invalid/check HTTP/1.1"
        );
        loop {
            line.clear();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
            if line == "\r\n" {
                break;
            }
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nproxy-ready").unwrap();
    };
    let (result, ()) = tokio::join!(request, serve);
    let result = result.unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "proxy-ready");
}

#[tokio::test]
#[ignore = "requires macOS Seatbelt and a working sandbox-exec"]
async fn macos_seatbelt_reaps_on_cancel_and_timeout() {
    assert!(availability().is_enforced());
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("build-test");
    std::fs::create_dir_all(&parent).unwrap();

    for cancel_requested in [true, false] {
        let workspace = tempfile::tempdir_in(&parent).unwrap();
        let cwd = workspace.path();
        let env = unix::execution_env(
            cwd,
            SandboxPolicy::for_workspace(cwd.to_path_buf()),
            None,
        );
        let cancel = CancellationToken::new();
        let running = env.exec(
            "sleep 30 & printf '%s\\n' \"$!\" > child.pid; wait",
            ShellExecOptions {
                cancel: Some(&cancel),
                timeout: Some(if cancel_requested { 12.0 } else { 5.0 }),
                ..ShellExecOptions::default()
            },
        );
        let observe = async {
            let pid = tokio::time::timeout(Duration::from_secs(4), async {
                loop {
                    if let Ok(value) =
                        std::fs::read_to_string(cwd.join("child.pid"))
                        && let Ok(pid) = value.trim().parse::<u32>()
                    {
                        break pid;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("sandbox reached the running child");
            assert!(process_exists(pid));
            if cancel_requested {
                cancel.cancel();
            }
            pid
        };
        let (result, pid) = tokio::join!(running, observe);
        let expected = if cancel_requested {
            ExecutionErrorCode::Aborted
        } else {
            ExecutionErrorCode::Timeout
        };
        assert_eq!(result.unwrap_err().code, expected);
        tokio::time::timeout(Duration::from_secs(5), async {
            while process_exists(pid) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("sandbox child was reaped");
    }
}

fn process_exists(pid: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
