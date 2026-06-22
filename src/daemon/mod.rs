pub mod pid;

use std::time::Duration;
use anyhow::Context;

use self::pid::{get_pid_path, is_running, process_exists, read_pid, remove_pid, stop_process, write_pid, get_log_path};

pub fn do_start(foreground: bool) -> anyhow::Result<()> {
    if is_running() {
        let pid = read_pid().unwrap();
        anyhow::bail!(
            "Server is already running with PID {}. Use 'stop' first or 'restart'.",
            pid
        );
    }

    if foreground {
        return Ok(());
    }

    let exe = std::env::current_exe().context("Cannot get executable path")?;

    let log_path = get_log_path();
    let log_file = std::fs::File::create(&log_path)
        .context("Cannot create log file")?;
    let err_file = log_file.try_clone()
        .context("Cannot clone log file handle")?;

    let child = std::process::Command::new(&exe)
        .arg("--foreground")
        .stdout(std::process::Stdio::from(log_file))
        .stderr(std::process::Stdio::from(err_file))
        .stdin(std::process::Stdio::null())
        .spawn()
        .context("Cannot spawn server process")?;

    let pid = child.id();
    write_pid(pid)?;

    std::thread::sleep(Duration::from_millis(500));

    if !process_exists(pid) {
        remove_pid().ok();
        anyhow::bail!("Server failed to start. Check the log file for details.");
    }

    println!("DriveShare started (PID: {})", pid);
    println!("Log file: {}", log_path.display());
    Ok(())
}

pub fn do_stop() -> anyhow::Result<()> {
    let pid = read_pid()?;

    if !process_exists(pid) {
        remove_pid().ok();
        anyhow::bail!("Process {} is not running", pid);
    }

    println!("Stopping DriveShare (PID: {})...", pid);
    stop_process(pid)?;

    std::thread::sleep(Duration::from_millis(500));

    remove_pid()?;
    println!("Server stopped.");
    Ok(())
}

pub fn do_status() -> anyhow::Result<()> {
    let path = get_pid_path();
    if !path.exists() {
        println!("DriveShare is not running.");
        return Ok(());
    }

    match read_pid() {
        Ok(pid) => {
            if process_exists(pid) {
                println!("DriveShare is running (PID: {})", pid);
            } else {
                println!("PID file exists but process {} is not running.", pid);
                println!("Use 'status --clean' to remove stale PID file.");
            }
        }
        Err(e) => {
            println!("PID file exists but could not be read: {}", e);
        }
    }
    Ok(())
}

pub fn clean_pid() -> anyhow::Result<()> {
    let pid = read_pid()?;
    if !process_exists(pid) {
        remove_pid()?;
        println!("Removed stale PID file for process {}", pid);
    } else {
        println!("Process {} is still running. Use 'stop' first.", pid);
    }
    Ok(())
}
