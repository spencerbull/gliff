use std::{net::SocketAddr, path::PathBuf};

use anyhow::{ensure, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Parser)]
#[command(
    version,
    about = "Gliff desktop client (US physical keyboard, CPU video)"
)]
pub struct Options {
    /// SSH config alias or user@host, using OpenSSH agent/key authentication.
    pub host: Option<String>,
    #[arg(long, default_value = "gliff-server")]
    pub server_bin: String,
    #[arg(long, conflicts_with = "output")]
    pub headless: bool,
    #[arg(long)]
    pub output: Option<String>,
    #[arg(long)]
    pub no_clipboard: bool,
    /// Unauthenticated development connection; literal loopback IP:port only.
    #[arg(long, conflicts_with = "host", value_parser = loopback)]
    pub connect: Option<SocketAddr>,
    /// Only CPU decode is supported; GPU/VA-API/Vulkan modes are unavailable.
    #[arg(long, default_value = "cpu", value_parser = ["cpu"])]
    pub video: String,
}

fn loopback(value: &str) -> Result<SocketAddr, String> {
    let addr: SocketAddr = value
        .parse()
        .map_err(|_| "use a literal loopback IP:port")?;
    if !addr.ip().is_loopback() || addr.port() == 0 {
        return Err("--connect is restricted to loopback development listeners".into());
    }
    Ok(addr)
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub hosts: Vec<String>,
    pub tabs: Vec<String>,
    pub selected: usize,
}

pub fn state_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let root = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    root.map(|p| p.join("Gliff").join("desktop.json"))
}

impl Saved {
    pub fn load(path: &std::path::Path) -> Self {
        // A corrupt or unexpectedly large state file must not prevent startup.
        let Ok(file) = std::fs::File::open(path) else {
            return Self::default();
        };
        use std::io::Read;
        let mut bytes = Vec::new();
        if file.take(64 * 1024 + 1).read_to_end(&mut bytes).is_err() || bytes.len() > 64 * 1024 {
            return Self::default();
        }
        let mut saved: Self = serde_json::from_slice(&bytes).unwrap_or_default();
        saved.hosts.retain(|h| crate::ssh::validate_host(h).is_ok());
        saved.hosts.truncate(16);
        saved.tabs.retain(|h| crate::ssh::validate_host(h).is_ok());
        saved.tabs.truncate(crate::session::MAX_TABS);
        saved.selected = saved.selected.min(saved.tabs.len().saturating_sub(1));
        saved
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        ensure!(
            self.hosts.len() <= 16 && self.tabs.len() <= crate::session::MAX_TABS,
            "too many saved hosts/tabs"
        );
        std::fs::create_dir_all(
            path.parent()
                .ok_or_else(|| anyhow::anyhow!("missing state directory"))?,
        )?;
        // State contains addresses only; no keys, passwords or clipboard contents.
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_remote_dev_connection_and_unsupported_video() {
        for addr in [
            "0.0.0.0:1234",
            "192.0.2.1:1234",
            "localhost:1234",
            "[::]:1234",
        ] {
            assert!(loopback(addr).is_err());
        }
        assert!(loopback("[::1]:1234").is_ok());
        assert!(Options::try_parse_from(["gliff-desktop", "--video", "gpu"]).is_err());
        assert!(
            Options::try_parse_from(["gliff-desktop", "--headless", "--output", "DP-1"]).is_err()
        );
    }
    #[test]
    fn persistence_round_trip_and_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        Saved {
            hosts: vec!["user@host".into()],
            tabs: vec!["user@host".into(), "second".into()],
            selected: 1,
        }
        .save(&path)
        .unwrap();
        let restored = Saved::load(&path);
        assert_eq!(restored.hosts, ["user@host"]);
        assert_eq!(restored.tabs, ["user@host", "second"]);
        assert_eq!(restored.selected, 1);
        std::fs::write(&path, b"bad").unwrap();
        assert!(Saved::load(&path).hosts.is_empty());
    }
}
