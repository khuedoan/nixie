//! Reading and hashing the installed machine ID over SSH.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use tracing::info_span;

use super::ssh_target;
use crate::hosts::hash_machine_id;

const MACHINE_ID_READ_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MACHINE_ID_READ_INTERVAL: Duration = Duration::from_secs(2);

pub fn read_machine_id_hash(
    user: &str,
    host: &str,
    ssh_key: &str,
    ssh_agent_socket: &str,
    debug: bool,
) -> Result<String> {
    read_machine_id_hash_timeout(
        user,
        host,
        ssh_key,
        ssh_agent_socket,
        debug,
        MACHINE_ID_READ_TIMEOUT,
    )
}

pub fn read_machine_id_hash_timeout(
    user: &str,
    host: &str,
    ssh_key: &str,
    ssh_agent_socket: &str,
    debug: bool,
    timeout: Duration,
) -> Result<String> {
    let _span = info_span!(
        "nixos.read_machine_id_hash",
        host,
        target = ssh_target(user, host)
    )
    .entered();

    if ssh_key.is_empty() && ssh_agent_socket.is_empty() {
        bail!("deployment SSH key or SSH agent socket is required");
    }

    let deadline = Instant::now() + timeout;
    let mut attempts = 0u32;

    loop {
        attempts += 1;
        match read_once(user, host, ssh_key, ssh_agent_socket, debug) {
            Ok(hash) => {
                tracing::debug!(attempts, hash, "read machine ID hash");
                return Ok(hash);
            }
            Err(error) => {
                tracing::debug!(attempts, %error, "machine ID read failed, retrying");
                if Instant::now() + MACHINE_ID_READ_INTERVAL >= deadline {
                    bail!("timed out reading final machine ID from {host}: {error:#}");
                }
            }
        }
        std::thread::sleep(MACHINE_ID_READ_INTERVAL);
    }
}

fn read_once(
    user: &str,
    host: &str,
    ssh_key: &str,
    ssh_agent_socket: &str,
    debug: bool,
) -> Result<String> {
    let mut args: Vec<String> = vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=5".into(),
        "-o".into(),
        "StrictHostKeyChecking=no".into(),
        "-o".into(),
        "UserKnownHostsFile=/dev/null".into(),
    ];
    if !ssh_key.is_empty() {
        args.push("-i".into());
        args.push(ssh_key.into());
    }
    args.push(ssh_target(user, host));
    args.push("cat /etc/machine-id".into());

    let mut command = Command::new("ssh");
    command.args(&args);
    super::anywhere::set_ssh_env(&mut command, ssh_agent_socket);
    if debug {
        command.stderr(Stdio::inherit());
    }

    let output = command.output().context("failed to run ssh")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        if message.is_empty() {
            bail!("ssh failed with {}", output.status);
        }
        bail!("ssh failed with {}: {message}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    hash_machine_id(&stdout).context("failed to hash machine ID")
}
