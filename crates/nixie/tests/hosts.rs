//! Host inventory round-trip and machine ID hashing.

use nixie::hosts::{self, State};

#[test]
fn hosts_config_round_trip_preserves_identity_and_drops_state() {
    let dir = std::env::temp_dir().join(format!("nixie-hosts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hosts.json");
    let original = r#"{
  "machine1": {
    "mac_address": "bc:24:11:d0:28:34",
    "ip": "192.168.1.42",
    "machine_id_hash": "abc123"
  }
}"#;
    std::fs::write(&path, original).unwrap();
    let path = path.to_string_lossy().to_string();

    let config = hosts::load_hosts_config(&path).unwrap();
    config.get("machine1").unwrap().set_state(State::Installing);
    hosts::save_hosts_config(&path, &config).unwrap();

    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(
        !saved.contains("State") && !saved.contains("installing"),
        "saved hosts file contains transient state: {saved}"
    );

    let config = hosts::load_hosts_config(&path).unwrap();
    let host = config.get("machine1").expect("machine1 present");
    assert_eq!(host.mac_address().to_string(), "bc:24:11:d0:28:34");
    assert_eq!(host.ip().as_deref(), Some("192.168.1.42"));
    assert_eq!(host.machine_id_hash().as_deref(), Some("abc123"));
    assert_eq!(host.state(), State::Unknown);
}

#[test]
fn hash_machine_id_is_keyed_lowercase_hex() {
    let machine_id = "0123456789abcdef0123456789abcdef";
    let hash = hosts::hash_machine_id(&format!("{machine_id}\n")).unwrap();
    assert_eq!(hash.len(), 64);
    assert_eq!(hash, hash.to_lowercase());
    assert_ne!(hash, machine_id);
    assert!(hosts::hash_machine_id("not-a-machine-id").is_err());
}

#[test]
fn finds_flake_output_by_mac_and_detects_all_installed() {
    let dir = std::env::temp_dir().join(format!("nixie-hosts-all-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hosts.json");
    std::fs::write(
        &path,
        r#"{"machine1": {"mac_address": "bc:24:11:d0:28:34"}}"#,
    )
    .unwrap();
    let path = path.to_string_lossy().to_string();

    let config = hosts::load_hosts_config(&path).unwrap();
    assert_eq!(
        hosts::get_flake_output_by_mac("BC:24:11:D0:28:34", &config).unwrap(),
        "machine1"
    );
    assert!(hosts::get_flake_output_by_mac("00:00:00:00:00:00", &config).is_err());
    assert!(!hosts::all_installed(&config));
    config.get("machine1").unwrap().set_state(State::Installed);
    assert!(hosts::all_installed(&config));
}
