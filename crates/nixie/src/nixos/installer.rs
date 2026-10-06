//! Building installer components from a flake output.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use tracing::{debug, info_span};

#[derive(Debug, Clone)]
pub struct InstallerComponents {
    pub kernel: String,
    pub initrd: String,
    pub init: String,
}

fn nix_build(flake_output: &str, debug: bool) -> Result<String> {
    let _span = info_span!("nix.build", "nix.flake_output" = flake_output).entered();

    let output = Command::new("nix")
        .args(["build", "--no-link", "--print-out-paths", flake_output])
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("failed to run nix build for {flake_output:?}"))?;

    if !output.status.success() {
        bail!("nix build failed for {flake_output:?}");
    }

    if debug {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn build_installer(flake_ref: &str, debug: bool) -> Result<InstallerComponents> {
    let _span = info_span!("nixie.build_installer", "nixie.installer" = flake_ref).entered();

    let kernel_out = nix_build(&format!("{flake_ref}.config.system.build.kernel"), debug)
        .context("failed to build kernel")?;
    let initrd_out = nix_build(
        &format!("{flake_ref}.config.system.build.netbootRamdisk"),
        debug,
    )
    .context("failed to build initrd")?;
    let toplevel_out = nix_build(&format!("{flake_ref}.config.system.build.toplevel"), debug)
        .context("failed to build toplevel")?;

    let components = InstallerComponents {
        kernel: join(&kernel_out, "bzImage"),
        initrd: join(&initrd_out, "initrd"),
        init: join(&toplevel_out, "init"),
    };
    debug!(?components, "installer components");
    Ok(components)
}

fn join(out: &str, name: &str) -> String {
    Path::new(out).join(name).to_string_lossy().into_owned()
}
