//! Running nixos-anywhere against a booted installer.

use std::process::{Command, Stdio};

use anyhow::{bail, Result};
use tracing::info_span;

use super::ssh_target;

pub fn install(
    flake_ref: &str,
    user: &str,
    host: &str,
    ssh_key: &str,
    ssh_agent_socket: &str,
    debug: bool,
) -> Result<()> {
    let target = ssh_target(user, host);
    let _span = info_span!("nixos.install", host, flake_ref, target).entered();

    if ssh_key.is_empty() && ssh_agent_socket.is_empty() {
        bail!("install SSH key or SSH agent socket is required");
    }

    let mut args: Vec<String> = vec![
        "--flake".into(),
        flake_ref.into(),
        "--target-host".into(),
        target,
        "--ssh-option".into(),
        "ConnectTimeout=10".into(),
        "--ssh-option".into(),
        "ServerAliveInterval=5".into(),
        "--ssh-option".into(),
        "ServerAliveCountMax=3".into(),
        "--ssh-option".into(),
        "StrictHostKeyChecking=no".into(),
        "--ssh-option".into(),
        "UserKnownHostsFile=/dev/null".into(),
        // Pushing from the local Nix store is usually faster than pulling the
        // closure over the internet, and it keeps the install air-gapped.
        "--no-substitute-on-destination".into(),
    ];
    if !ssh_key.is_empty() {
        args.push("-i".into());
        args.push(ssh_key.into());
    }

    let mut command = Command::new("nixos-anywhere");
    command.args(&args);
    set_ssh_env(&mut command, ssh_agent_socket);
    if debug {
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }

    let status = command.status()?;
    if !status.success() {
        bail!("nixos-anywhere failed with {status}");
    }
    Ok(())
}

/// Add `SSH_AUTH_SOCK` to the child environment when one is configured.
pub(crate) fn set_ssh_env(command: &mut Command, ssh_agent_socket: &str) {
    if !ssh_agent_socket.is_empty() {
        command.env("SSH_AUTH_SOCK", ssh_agent_socket);
    }
}
