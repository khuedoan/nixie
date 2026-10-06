//! NixOS build and installation helpers.

pub mod anywhere;
pub mod installer;
pub mod machine_id;

pub use anywhere::install;
pub use installer::{build_installer, InstallerComponents};
pub use machine_id::read_machine_id_hash;

/// Format an SSH target, wrapping IPv6 literals in brackets.
pub(crate) fn ssh_target(user: &str, host: &str) -> String {
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    format!("{user}@{host}")
}
