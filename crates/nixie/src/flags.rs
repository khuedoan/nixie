//! Command-line flags.

use anyhow::{bail, Result};
use clap::Parser;

#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(name = "nixie", about = "Bare metal provisioning engine for NixOS")]
pub struct Flags {
    /// Enable debug logging.
    #[arg(long, default_value_t = false)]
    pub debug: bool,

    /// Address to listen on (default auto).
    #[arg(long, default_value = "")]
    pub address: String,

    /// Path to the SSH private key authorized by the installed system.
    #[arg(long = "deployment-ssh-key", default_value = "")]
    pub deployment_ssh_key: String,

    /// SSH user for the installed system.
    #[arg(long = "deployment-ssh-user", default_value = "root")]
    pub deployment_ssh_user: String,

    /// NixOS configuration flake (for example, .).
    #[arg(long, default_value = "")]
    pub flake: String,

    /// Path to hosts.json file (for example, ./hosts.json).
    #[arg(long, default_value = "")]
    pub hosts: String,

    /// Path to the SSH private key authorized by the installer.
    #[arg(long = "install-ssh-key", default_value = "")]
    pub install_ssh_key: String,

    /// NixOS installer flake output (for example, .#nixosConfigurations.installer).
    #[arg(long, default_value = "")]
    pub installer: String,

    /// SSH agent socket (defaults to $SSH_AUTH_SOCK); allows omitting both SSH key flags.
    #[arg(long = "ssh-agent-socket", env = "SSH_AUTH_SOCK", default_value = "")]
    pub ssh_agent_socket: String,
}

impl Flags {
    pub fn parse_args() -> Result<Self> {
        Self::parse_args_from(std::env::args_os())
    }

    pub fn parse_args_from<I, T>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        match Flags::try_parse_from(args) {
            Ok(flags) => {
                flags.validate()?;
                Ok(flags)
            }
            // Help and version output go to stdout and should exit cleanly.
            Err(error) if !error.use_stderr() => error.exit(),
            Err(error) => Err(error.into()),
        }
    }

    fn validate(&self) -> Result<()> {
        if self.hosts.is_empty() || self.flake.is_empty() || self.installer.is_empty() {
            bail!(
                "missing flags, usage: nixie --hosts <hosts.json> --flake <flake> \
                 --installer <installer-output> [--ssh-agent-socket <socket> | \
                 --install-ssh-key <private-key> --deployment-ssh-key <private-key>]"
            );
        }
        if self.ssh_agent_socket.is_empty()
            && (self.install_ssh_key.is_empty() || self.deployment_ssh_key.is_empty())
        {
            bail!(
                "SSH authentication requires SSH_AUTH_SOCK, --ssh-agent-socket, or both \
                 --install-ssh-key and --deployment-ssh-key"
            );
        }
        Ok(())
    }
}
