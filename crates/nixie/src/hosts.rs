//! Declarative JSON host inventory.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use hmac::{Hmac, Mac};
use serde::de::{self, Deserializer};
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::mac::MacAddr;

type HmacSha256 = Hmac<Sha256>;

/// Installation state of a host. Not persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Unknown,
    Installing,
    Installed,
    Failed,
}

/// A machine to provision.
#[derive(Debug)]
pub struct Host {
    mac_address: MacAddr,
    inner: Mutex<HostInner>,
}

#[derive(Debug, Default)]
struct HostInner {
    ip: Option<String>,
    machine_id_hash: Option<String>,
    state: State,
}

impl Host {
    pub fn mac_address(&self) -> MacAddr {
        self.mac_address
    }

    pub fn ip(&self) -> Option<String> {
        self.inner.lock().unwrap().ip.clone()
    }

    pub fn machine_id_hash(&self) -> Option<String> {
        self.inner.lock().unwrap().machine_id_hash.clone()
    }

    pub fn state(&self) -> State {
        self.inner.lock().unwrap().state
    }

    pub fn set_state(&self, state: State) {
        self.inner.lock().unwrap().state = state;
    }

    pub fn set_final_identity(&self, ip: String, machine_id_hash: String) {
        let mut inner = self.inner.lock().unwrap();
        inner.ip = Some(ip);
        inner.machine_id_hash = Some(machine_id_hash);
    }

    /// Persisted identity, matching the Go fields that use `omitempty`.
    fn persisted(&self) -> PersistedHost {
        let inner = self.inner.lock().unwrap();
        PersistedHost {
            mac_address: self.mac_address.to_string(),
            ip: inner.ip.clone(),
            machine_id_hash: inner.machine_id_hash.clone(),
        }
    }
}

#[derive(Serialize)]
struct PersistedHost {
    mac_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    machine_id_hash: Option<String>,
}

impl Serialize for Host {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let persisted = self.persisted();
        let fields = 1
            + usize::from(persisted.ip.is_some())
            + usize::from(persisted.machine_id_hash.is_some());
        let mut state = serializer.serialize_struct("Host", fields)?;
        state.serialize_field("mac_address", &persisted.mac_address)?;
        if let Some(ip) = &persisted.ip {
            state.serialize_field("ip", ip)?;
        }
        if let Some(hash) = &persisted.machine_id_hash {
            state.serialize_field("machine_id_hash", hash)?;
        }
        state.end()
    }
}

#[derive(Deserialize)]
struct RawHost {
    mac_address: String,
    #[serde(default)]
    ip: String,
    #[serde(default)]
    machine_id_hash: String,
}

impl<'de> Deserialize<'de> for Host {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawHost::deserialize(deserializer)?;
        let mac_address = MacAddr::from_str(raw.mac_address.trim()).map_err(|e| {
            de::Error::custom(format!("invalid MAC address {:?}: {e}", raw.mac_address))
        })?;

        let ip = if raw.ip.trim().is_empty() {
            None
        } else {
            let parsed: std::net::IpAddr = raw.ip.trim().parse().map_err(de::Error::custom)?;
            Some(parsed.to_string())
        };
        let hash = raw.machine_id_hash.trim();
        let machine_id_hash = (!hash.is_empty()).then(|| hash.to_string());

        Ok(Host {
            mac_address,
            inner: Mutex::new(HostInner {
                ip,
                machine_id_hash,
                state: State::Unknown,
            }),
        })
    }
}

/// Host inventory keyed by flake output name, ordered for stable JSON output.
pub type HostsConfig = BTreeMap<String, Arc<Host>>;

pub fn load_hosts_config(filename: &str) -> Result<HostsConfig> {
    let data = std::fs::read_to_string(filename).context("failed to read hosts file")?;
    let config = serde_json::from_str(&data).context("failed to parse hosts file")?;
    Ok(config)
}

pub fn save_hosts_config(filename: &str, config: &HostsConfig) -> Result<()> {
    let mut data = serde_json::to_string_pretty(config).context("failed to encode hosts file")?;
    data.push('\n');
    std::fs::write(filename, data).context("failed to write hosts file")?;
    Ok(())
}

/// Hash a systemd machine ID with an app-specific key.
///
/// systemd treats `/etc/machine-id` as confidential, so we store a keyed hash
/// instead of the raw ID.
pub fn hash_machine_id(machine_id: &str) -> Result<String> {
    let machine_id = machine_id.trim().to_lowercase();
    if machine_id.len() != 32 {
        bail!(
            "invalid machine ID length: got {}, want 32",
            machine_id.len()
        );
    }
    if hex::decode(&machine_id).is_err() {
        bail!("invalid machine ID {machine_id:?}");
    }

    let mut mac = HmacSha256::new_from_slice(b"code.khuedoan.com/nixie/machine-id/v1")
        .expect("HMAC accepts any key length");
    mac.update(machine_id.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

pub fn get_flake_output_by_mac(mac_address: &str, config: &HostsConfig) -> Result<String> {
    for (flake, host) in config {
        if host
            .mac_address()
            .to_string()
            .eq_ignore_ascii_case(mac_address)
        {
            return Ok(flake.clone());
        }
    }
    bail!("unknown MAC address: {mac_address}")
}

pub fn all_installed(config: &HostsConfig) -> bool {
    config.values().all(|host| host.state() == State::Installed)
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            State::Unknown => "unknown",
            State::Installing => "installing",
            State::Installed => "installed",
            State::Failed => "failed",
        };
        f.write_str(name)
    }
}
