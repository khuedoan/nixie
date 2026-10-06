//! Nixie: provision NixOS onto bare metal machines.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use nixie::api::{start_api_server, Api, ApiConfig};
use nixie::flags::Flags;
use nixie::hosts::{self, State};
use nixie::netboot::dhcp::Firmware;
use nixie::network;
use nixie::nixos;
use nixie::otel;
use nixie::pxe::{NixieBooter, PxeServer, IPXE_EFI64};
use tracing::{debug, error, info, warn};

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_signal: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}

fn install_signal_handlers() {
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
    }
}

fn main() -> ExitCode {
    let flags = match Flags::parse_args() {
        Ok(flags) => flags,
        Err(error) => {
            eprintln!("failed to parse command-line flags: {error:#}");
            return ExitCode::FAILURE;
        }
    };
    otel::init(flags.debug);
    match run(&flags) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(flags: &Flags) -> anyhow::Result<()> {
    let hosts_config = hosts::load_hosts_config(&flags.hosts)?;
    debug!(?flags, "parsed command line flags");

    let address = if flags.address.is_empty() {
        network::detect_server_address()?
    } else {
        flags.address.clone()
    };
    debug!(%address, "server address");
    install_signal_handlers();

    check_installed_hosts(
        &hosts_config,
        &flags.deployment_ssh_user,
        &flags.deployment_ssh_key,
        &flags.ssh_agent_socket,
        flags.debug,
    );
    if hosts::all_installed(&hosts_config) {
        info!("all hosts are already installed");
        return Ok(());
    }

    info!(installer = %flags.installer, "building installer");
    let components = nixos::build_installer(&flags.installer, flags.debug)?;
    PxeServer::validate_files(&[
        components.kernel.clone(),
        components.initrd.clone(),
        components.init.clone(),
    ])?;

    let address: IpAddr = address.parse()?;
    let booter = NixieBooter {
        address: address.to_string(),
        kernel: components.kernel,
        initrd: components.initrd,
        init: components.init,
        hosts_config: hosts_config.clone(),
    };
    let mut ipxe = BTreeMap::new();
    ipxe.insert(Firmware::EFI64, IPXE_EFI64.to_vec());
    let server = Arc::new(PxeServer::new(address, Arc::new(booter), ipxe));

    let serve_handle = {
        let server = Arc::clone(&server);
        std::thread::spawn(move || server.serve())
    };
    info!(%address, "PXE server started");

    let (done_tx, done_rx) = mpsc::channel();
    let api = Api::new(ApiConfig {
        hosts_config: hosts_config.clone(),
        hosts_file: flags.hosts.clone(),
        flake: flags.flake.clone(),
        install_ssh_key: flags.install_ssh_key.clone(),
        deployment_ssh_user: flags.deployment_ssh_user.clone(),
        deployment_ssh_key: flags.deployment_ssh_key.clone(),
        ssh_agent_socket: flags.ssh_agent_socket.clone(),
        debug: flags.debug,
        done_tx,
    });
    std::thread::spawn(move || {
        if let Err(error) = start_api_server(api) {
            error!(%error, "API server failed");
        }
    });

    for (name, host) in &hosts_config {
        if host.state() == State::Installed {
            debug!(%name, mac = %host.mac_address(), "skipping installed host");
            continue;
        }
        info!(mac = %host.mac_address(), "sending magic packet");
        if let Err(error) = network::send_wake_on_lan(host.mac_address().as_bytes()) {
            warn!(%error, "failed to send magic packet");
        }
    }

    loop {
        if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
            info!("signal received, shutting down");
            break;
        }
        if done_rx.try_recv().is_ok() {
            info!("all hosts installed, shutting down");
            break;
        }
        if serve_handle.is_finished() {
            warn!("PXE server stopped unexpectedly");
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    server.shutdown();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !serve_handle.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
    if serve_handle.is_finished() {
        match serve_handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => warn!(%error, "PXE server stopped with error"),
            Err(_) => warn!("PXE server thread panicked"),
        }
    } else {
        warn!("timed out waiting for PXE server to stop");
    }

    info!("nixie stopped gracefully");
    Ok(())
}

fn check_installed_hosts(
    config: &hosts::HostsConfig,
    deployment_ssh_user: &str,
    deployment_ssh_key: &str,
    ssh_agent_socket: &str,
    debug: bool,
) {
    for (name, host) in config {
        let Some(stored_hash) = host.machine_id_hash() else {
            continue;
        };

        host.set_state(State::Installed);
        let Some(stored_ip) = host.ip() else {
            warn!(%name, mac = %host.mac_address(), "installed host has no IP, skipping status check");
            continue;
        };

        match nixos::machine_id::read_machine_id_hash_timeout(
            deployment_ssh_user,
            &stored_ip,
            deployment_ssh_key,
            ssh_agent_socket,
            debug,
            Duration::from_secs(15),
        ) {
            Ok(hash) if hash == stored_hash => {
                info!(%name, ip = %stored_ip, "installed host verified, skipping reinstall");
            }
            Ok(hash) => {
                warn!(%name, ip = %stored_ip, expected = %stored_hash, actual = %hash, "machine ID hash mismatch, skipping reinstall");
            }
            Err(error) => {
                warn!(%name, ip = %stored_ip, %error, "failed to check installed host, skipping reinstall");
            }
        }
    }
}
