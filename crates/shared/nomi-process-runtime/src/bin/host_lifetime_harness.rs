//! Spawns an unmanaged helper tree through `hidden_std_command`, then exits
//! without dropping the child so the host-lifetime Job / process-group reaper
//! is the only cleanup path.

#[cfg(any(unix, windows))]
use std::{
    env,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    thread,
    time::{Duration, Instant},
};

#[cfg(any(unix, windows))]
use nomi_process_runtime::{hidden_std_command, install_host_process_lifetime};

#[cfg(not(any(unix, windows)))]
fn main() {}

#[cfg(any(unix, windows))]
fn main() {
    if let Err(error) = run() {
        eprintln!("host-lifetime harness failed: {error}");
        process::exit(2);
    }
}

#[cfg(any(unix, windows))]
fn run() -> io::Result<()> {
    install_host_process_lifetime();

    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 5 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected helper, leader marker, grandchild marker, start gate, and exit gate",
        ));
    }
    let helper = &args[0];
    let leader_marker = PathBuf::from(&args[1]);
    let grandchild_marker = PathBuf::from(&args[2]);
    let start_gate = PathBuf::from(&args[3]);
    let exit_gate = PathBuf::from(&args[4]);

    wait_for_file(&start_gate, "start")?;

    let child = hidden_std_command(helper)
        .arg("spawn-grandchild")
        .arg(&grandchild_marker)
        .spawn()?;
    write_pid_atomically(&leader_marker, child.id())?;
    wait_for_file(&grandchild_marker, "grandchild")?;
    wait_for_file(&exit_gate, "exit")?;

    std::mem::forget(child);
    abrupt_exit();
}

#[cfg(windows)]
fn abrupt_exit() -> ! {
    use windows_sys::Win32::System::Threading::ExitProcess;
    // SAFETY: the harness exits while the host-lifetime Job still owns the
    // helper tree. ExitProcess skips Rust drops; kernel close of the Job
    // handle is the behavior under test.
    unsafe { ExitProcess(0) }
}

#[cfg(unix)]
fn abrupt_exit() -> ! {
    // SAFETY: the harness exits without dropping the child or the reaper so
    // parent-death delivery (PDEATHSIG / sibling reaper) is the cleanup path.
    unsafe { libc::_exit(0) }
}

#[cfg(any(unix, windows))]
fn wait_for_file(path: &Path, label: &str) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if path.is_file() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("{label} gate timed out: {}", path.display()),
    ))
}

#[cfg(any(unix, windows))]
fn write_pid_atomically(path: &Path, pid: u32) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "marker has no file name"))?;
    let harness_pid = process::id();
    for attempt in 0..100_u32 {
        let mut temporary_name = OsString::from(".");
        temporary_name.push(file_name);
        temporary_name.push(format!(".{harness_pid}.{attempt}.tmp"));
        let temporary_path = parent.join(temporary_name);
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
        {
            Ok(mut file) => {
                let result = (|| {
                    file.write_all(pid.to_string().as_bytes())?;
                    file.write_all(b"\n")?;
                    file.flush()?;
                    file.sync_all()?;
                    drop(file);
                    fs::rename(&temporary_path, path)
                })();
                if result.is_err() {
                    let _ = fs::remove_file(&temporary_path);
                }
                return result;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a temporary marker",
    ))
}
