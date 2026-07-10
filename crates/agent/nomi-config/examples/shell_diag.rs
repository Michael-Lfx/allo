#[tokio::main]
async fn main() {
    let marker = std::env::temp_dir().join("nomi_shell_sidechannel.txt");
    let _ = std::fs::remove_file(&marker);
    let cmd = format!(
        "Write-Output 'hello_diag'; 'side' | Set-Content -Encoding utf8 '{}'",
        marker.display().to_string().replace('\\', "\\\\")
    );
    println!("running command via shell_command_builder...");
    let args = nomi_config::shell::shell_command_args(&cmd);
    println!("args count={}", args.len());
    for (i, a) in args.iter().enumerate() {
        println!("arg[{i}] bytes={} chars={}", a.len(), a.chars().count());
        if i == args.len() - 1 {
            println!("--- payload start ---");
            println!("{a}");
            println!("--- payload end ---");
        } else {
            println!("arg[{i}]={a:?}");
        }
    }
    let out = nomi_config::shell::shell_command_builder(&cmd)
        .output()
        .await
        .expect("spawn failed");
    println!("status={:?}", out.status.code());
    println!("stdout_len={} stdout={:?}", out.stdout.len(), String::from_utf8_lossy(&out.stdout));
    println!("stderr_len={} stderr={:?}", out.stderr.len(), String::from_utf8_lossy(&out.stderr));
    println!("marker_exists={} content={:?}", marker.exists(), std::fs::read_to_string(&marker).ok());
}
