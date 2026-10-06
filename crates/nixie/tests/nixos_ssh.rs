//! SSH invocation tests using fake `ssh` and `nixos-anywhere` commands.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use nixie::nixos;

const TEST_MACHINE_ID: &str = "0123456789abcdef0123456789abcdef";
const TEST_MACHINE_ID_HASH: &str =
    "dada2cfcc22d3f6285b65cf851d8582adeeb8d70f7f0fcfe9402cbef732b23ec";

fn install_fake_command(directory: &Path, name: &str, command: &str) -> PathBuf {
    let capture_file = directory.join("invocation");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$SSH_AUTH_SOCK\" > \"$CAPTURE_FILE\"\nprintf '%s\\n' \"$@\" >> \"$CAPTURE_FILE\"\n{command}\n"
    );
    let path = directory.join(name);
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    capture_file
}

fn invocation_lines(capture_file: &Path) -> Vec<String> {
    fs::read_to_string(capture_file)
        .unwrap()
        .trim_end_matches('\n')
        .split('\n')
        .map(str::to_string)
        .collect()
}

fn assert_ssh_invocation(capture_file: &Path, socket: &str, key: &str) {
    let lines = invocation_lines(capture_file);
    assert_eq!(lines[0], socket, "SSH_AUTH_SOCK");
    let arguments = &lines[1..];

    let identity_file = arguments
        .iter()
        .position(|argument| argument == "-i")
        .map(|index| arguments.get(index + 1).cloned().unwrap_or_default());
    if key.is_empty() {
        assert!(
            identity_file.is_none(),
            "agent-only invocation contains -i: {arguments:?}"
        );
    } else {
        assert_eq!(
            identity_file.as_deref(),
            Some(key),
            "arguments: {arguments:?}"
        );
    }
}

#[test]
fn ssh_commands_use_agent_or_key() {
    let directory = std::env::temp_dir().join(format!("nixie-ssh-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();

    // Restore the environment even if an assertion fails.
    let original_path = std::env::var("PATH").unwrap_or_default();
    let guard = PathGuard(original_path);

    std::env::set_var("PATH", format!("{}:{}", directory.display(), guard.0));
    let socket = "/run/user/1000/ssh-agent.sock";
    let key = "/keys/id_ed25519";

    // Agent only, no identity file.
    let capture = install_fake_command(&directory, "nixos-anywhere", "exit 0");
    std::env::set_var("SSH_AUTH_SOCK", socket);
    std::env::set_var("CAPTURE_FILE", &capture);
    nixos::install(".#claw", "root", "192.0.2.1", "", socket, false).unwrap();
    assert_ssh_invocation(&capture, socket, "");

    // Explicit identity file.
    let capture = install_fake_command(&directory, "nixos-anywhere", "exit 0");
    std::env::set_var("CAPTURE_FILE", &capture);
    std::env::set_var("SSH_AUTH_SOCK", "");
    nixos::install(".#claw", "root", "192.0.2.1", key, "", false).unwrap();
    assert_ssh_invocation(&capture, "", key);

    // Machine ID read with agent, and the hash must match Go's HMAC.
    let capture = install_fake_command(
        &directory,
        "ssh",
        &format!("printf '%s\\n' {TEST_MACHINE_ID}"),
    );
    std::env::set_var("CAPTURE_FILE", &capture);
    std::env::set_var("SSH_AUTH_SOCK", socket);
    let hash = nixos::read_machine_id_hash("root", "192.0.2.1", "", socket, false).unwrap();
    assert_eq!(hash, TEST_MACHINE_ID_HASH);
    assert_ssh_invocation(&capture, socket, "");

    // Machine ID read with an explicit key.
    let capture = install_fake_command(
        &directory,
        "ssh",
        &format!("printf '%s\\n' {TEST_MACHINE_ID}"),
    );
    std::env::set_var("CAPTURE_FILE", &capture);
    std::env::set_var("SSH_AUTH_SOCK", "");
    let hash = nixos::read_machine_id_hash("root", "192.0.2.1", key, "", false).unwrap();
    assert_eq!(hash, TEST_MACHINE_ID_HASH);
    assert_ssh_invocation(&capture, "", key);
}

struct PathGuard(String);

impl Drop for PathGuard {
    fn drop(&mut self) {
        std::env::set_var("PATH", &self.0);
    }
}
