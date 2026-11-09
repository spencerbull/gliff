//! GUI-safe OpenSSH launch. The legacy GTK helper has a different TOFU policy.
use crate::config::Options;
use anyhow::{ensure, Result};
use tokio::process::Command;

pub fn validate_host(host: &str) -> Result<()> {
    ensure!(
        !host.is_empty()
            && host.len() <= 255
            && !host.starts_with('-')
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "@._-:[]".contains(c)),
        "Use an SSH config alias or user@host (no options or whitespace)"
    );
    Ok(())
}

fn quote(value: &str) -> Result<String> {
    ensure!(
        !value.is_empty() && value.len() <= 4096 && !value.contains(['\0', '\n', '\r']),
        "Invalid remote command argument"
    );
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

pub fn command(host: &str, options: &Options) -> Result<Command> {
    validate_host(host)?;
    let mut remote = format!("{} --stdio", quote(&options.server_bin)?);
    if options.headless {
        remote.push_str(" --headless");
    }
    if let Some(output) = &options.output {
        remote.push_str(" --output ");
        remote.push_str(&quote(output)?);
    }
    let mut cmd = Command::new("ssh");
    cmd.args([
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "PasswordAuthentication=no",
        "-o",
        "KbdInteractiveAuthentication=no",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ConnectionAttempts=1",
        "-o",
        "ServerAliveInterval=5",
        "-o",
        "ServerAliveCountMax=2",
        "-o",
        "Compression=no",
        "--",
        host,
        &remote,
    ]);
    cmd.env("SSH_ASKPASS_REQUIRE", "never");
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW; never open a password console.
    Ok(cmd)
}

pub fn diagnostic(stderr: &str) -> String {
    format!("{}\nCheck Windows OpenSSH is installed, the host is verified in known_hosts, your agent/key works with `ssh -o BatchMode=yes HOST`, and gliff-server is installed on the remote PATH (or set --server-bin). Password prompts are disabled.", stderr.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn rejects_option_injection_and_quotes_remote_shell_arguments() {
        for host in [
            "-oProxyCommand=evil",
            "host;evil",
            "user@host x",
            "$(evil)",
            "",
        ] {
            assert!(validate_host(host).is_err());
        }
        let opts = Options::parse_from(["gliff-desktop", "--server-bin", "/tmp/a'b;echo bad"]);
        let cmd = command("user@host", &opts).unwrap();
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|v| v.to_str().unwrap())
            .collect();
        assert!(args.contains(&"BatchMode=yes"));
        assert!(args.contains(&"StrictHostKeyChecking=yes"));
        assert!(!args.iter().any(|s| s.contains("accept-new")));
        assert_eq!(args.last().unwrap(), &"'/tmp/a'\\''b;echo bad' --stdio");
    }
}
