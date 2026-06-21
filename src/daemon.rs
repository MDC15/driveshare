use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use anyhow::Context;

pub fn get_pid_path() -> PathBuf {
    let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    path.push("driveshare.pid");
    path
}

pub fn get_log_path() -> PathBuf {
    let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    path.push("driveshare.log");
    path
}

pub fn write_pid(pid: u32) -> anyhow::Result<()> {
    let path = get_pid_path();
    fs::write(&path, pid.to_string())?;
    Ok(())
}

pub fn read_pid() -> anyhow::Result<u32> {
    let path = get_pid_path();
    if !path.exists() {
        anyhow::bail!("PID file not found. Is the server running?");
    }
    let content = fs::read_to_string(&path)?;
    let pid = content.trim().parse::<u32>()
        .map_err(|_| anyhow::anyhow!("Invalid PID in file: {}", content.trim()))?;
    Ok(pid)
}

pub fn remove_pid() -> anyhow::Result<()> {
    let path = get_pid_path();
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(unix)]
pub fn process_exists(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
pub fn process_exists(pid: u32) -> bool {
    let output = std::process::Command::new("tasklist")
        .arg("/FI")
        .arg(format!("PID eq {}", pid))
        .output()
        .ok();
    match output {
        Some(o) => {
            let out = String::from_utf8_lossy(&o.stdout);
            out.contains(&pid.to_string())
        }
        None => false,
    }
}

#[cfg(unix)]
pub fn stop_process(pid: u32) -> anyhow::Result<()> {
    let status = std::process::Command::new("kill")
        .arg(pid.to_string())
        .status()?;
    if !status.success() {
        anyhow::bail!("kill command failed for PID {}", pid);
    }
    Ok(())
}

#[cfg(windows)]
pub fn stop_process(pid: u32) -> anyhow::Result<()> {
    let status = std::process::Command::new("taskkill")
        .arg("/F")
        .arg("/PID")
        .arg(pid.to_string())
        .status()?;
    if !status.success() {
        anyhow::bail!("taskkill failed for PID {}", pid);
    }
    Ok(())
}

pub fn is_running() -> bool {
    match read_pid() {
        Ok(pid) => process_exists(pid),
        Err(_) => false,
    }
}

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
    let log_file = fs::File::create(&log_path)
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
