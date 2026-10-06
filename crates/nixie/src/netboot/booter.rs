//! Booter interface and shared boot types.

use std::fs::File;

use anyhow::Result;

use crate::mac::MacAddr;

/// CPU architecture reported by the client, matching Go's `Architecture`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    IA32,
    X64,
}

impl Architecture {
    /// Numeric value embedded in the iPXE boot script URL.
    pub fn number(self) -> u8 {
        match self {
            Architecture::IA32 => 0,
            Architecture::X64 => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Architecture::IA32 => "IA32",
            Architecture::X64 => "X64",
        }
    }
}

/// A machine attempting to boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Machine {
    pub mac: MacAddr,
    pub arch: Architecture,
}

/// Boot instructions for a machine, matching Pixiecore's `Spec`.
#[derive(Debug, Clone, Default)]
pub struct BootSpec {
    pub kernel: String,
    pub initrd: Vec<String>,
    pub cmdline: String,
    pub message: String,
    pub ipxe_script: String,
}

/// Provides boot instructions and files.
pub trait Booter: Send + Sync {
    /// What should this machine boot? `Ok(None)` (or an error) ignores the request.
    fn boot_spec(&self, machine: Machine) -> Result<Option<BootSpec>>;
    /// Open a boot file by ID, returning the reader and its size.
    fn read_boot_file(&self, id: &str) -> Result<(File, u64)>;
}
