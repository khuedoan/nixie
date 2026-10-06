//! Kernel command line parsing.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub mac_address: String,
    pub api_address: String,
}

pub fn get_agent_config() -> Result<AgentConfig, String> {
    let cmdline = std::fs::read_to_string("/proc/cmdline")
        .map_err(|error| format!("failed to read /proc/cmdline: {error}"))?;

    let params = parse_kernel_params(&cmdline);
    let mac_address = params.get("nixie_mac_address").cloned().unwrap_or_default();
    let api_address = params.get("nixie_api").cloned().unwrap_or_default();
    if mac_address.is_empty() || api_address.is_empty() {
        return Err(
            "missing required kernel parameters: nixie_mac_address or nixie_api".to_string(),
        );
    }

    Ok(AgentConfig {
        mac_address,
        api_address,
    })
}

pub fn parse_kernel_params(cmdline: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    for field in cmdline.split_whitespace() {
        if let Some((key, value)) = field.split_once('=') {
            params.insert(key.to_string(), value.to_string());
        }
    }
    params
}
