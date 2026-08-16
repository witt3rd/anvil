//! systemd user unit: serve comes back after login/reboot, still cold.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

pub const UNIT_NAME: &str = "anvil.service";
pub const UNIT_BODY: &str = include_str!("../../systemd/anvil.service");

pub fn unit_path() -> PathBuf {
    dirs_config()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("systemd/user")
        .join(UNIT_NAME)
}

fn dirs_config() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
}

pub fn install() -> io::Result<PathBuf> {
    let path = unit_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, UNIT_BODY)?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", UNIT_NAME])?;
    enable_linger()?;
    Ok(path)
}

pub fn enable_linger() -> io::Result<()> {
    let user = std::env::var("USER").map_err(io::Error::other)?;
    let status = Command::new("loginctl")
        .args(["enable-linger", &user])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "loginctl enable-linger {user} failed"
        )))
    }
}

pub fn linger_on() -> bool {
    let Ok(user) = std::env::var("USER") else {
        return false;
    };
    let out = Command::new("loginctl")
        .args(["show-user", &user, "-p", "Linger", "--value"])
        .output();
    matches!(out, Ok(o) if String::from_utf8_lossy(&o.stdout).trim() == "yes")
}

pub fn uninstall() -> io::Result<()> {
    let _ = systemctl(&["disable", "--now", UNIT_NAME]);
    let path = unit_path();
    if path.is_file() {
        fs::remove_file(path)?;
    }
    let _ = systemctl(&["daemon-reload"]);
    Ok(())
}

pub fn enabled() -> bool {
    systemctl(&["is-enabled", "--quiet", UNIT_NAME]).is_ok()
}

pub fn active() -> bool {
    systemctl(&["is-active", "--quiet", UNIT_NAME]).is_ok()
}

fn systemctl(args: &[&str]) -> io::Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "systemctl --user {} failed",
            args.join(" ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_file_starts_the_wrapper_and_is_wanted_by_default() {
        assert!(UNIT_BODY.contains("ExecStart=%h/.local/bin/anvil serve"));
        assert!(UNIT_BODY.contains("ExecStop=%h/.local/bin/anvil serve --stop"));
        assert!(UNIT_BODY.contains("WantedBy=default.target"));
        assert!(UNIT_BODY.contains("Restart=on-failure"));
    }
}
