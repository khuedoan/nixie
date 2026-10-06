//! Nixie installer agent.

use nixie_agent::{client, kernel};

fn main() {
    let params = match kernel::get_agent_config() {
        Ok(params) => params,
        Err(error) => {
            eprintln!("failed to get Nixie params: {error}");
            std::process::exit(1);
        }
    };
    println!(
        "nixie-agent params: mac_address={} api_address={}",
        params.mac_address, params.api_address
    );

    if let Err(error) = client::ping(&params.api_address) {
        eprintln!("failed to ping Nixie API server: {error}");
        std::process::exit(1);
    }
    println!("successfully sent ping to API server");

    if let Err(error) = client::install(&params.api_address, &params.mac_address) {
        eprintln!("failed to request for installation: {error}");
        std::process::exit(1);
    }
}
