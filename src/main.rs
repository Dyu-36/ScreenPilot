#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() {
    if let Err(err) = run() {
        let _ = write_error_log(&err.to_string());
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_output_path);

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    capture_screen(&output)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn capture_screen(output: &Path) -> Result<(), Box<dyn Error>> {
    use xcap::Monitor;

    let monitors = Monitor::all()?;
    if monitors.is_empty() {
        return Err("No monitor found".into());
    }

    let monitor = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .unwrap_or(&monitors[0]);
    let image = monitor.capture_image()?;
    image.save(output)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn capture_screen(output: &Path) -> Result<(), Box<dyn Error>> {
    use std::process::{Command, Stdio};

    let output = output.to_str().ok_or("Invalid output path")?;
    let status = Command::new("spectacle")
        .arg("--background")
        .arg("--nonotify")
        .arg("--fullscreen")
        .arg("--output")
        .arg(output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;

    if !status.success() {
        return Err(format!("Spectacle failed with exit code: {:?}", status.code()).into());
    }
    if !Path::new(output).exists() {
        return Err("Screenshot file was not created".into());
    }
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn capture_screen(_output: &Path) -> Result<(), Box<dyn Error>> {
    Err("Unsupported operating system".into())
}

fn default_output_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    executable_directory().join(format!("screenshot-{timestamp}.png"))
}

fn executable_directory() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn write_error_log(message: &str) -> std::io::Result<()> {
    let log_path = executable_directory().join("screenshot-error.log");
    fs::write(log_path, message)
}
