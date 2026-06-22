use std::fs;
use std::path::PathBuf;

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
