//! The HTTP API the installed agent talks to.

use std::net::SocketAddr;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde::Deserialize;
use tiny_http::{Header, Method, Response};
use tracing::{debug, error, info, info_span, warn};

use crate::hosts::{self, Host, HostsConfig, State};
use crate::nixos;

pub const API_PORT: u16 = 5000;

#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    pub mac_address: String,
}

/// Everything the API needs to drive installs.
pub struct ApiConfig {
    pub hosts_config: HostsConfig,
    pub hosts_file: String,
    pub flake: String,
    pub install_ssh_key: String,
    pub deployment_ssh_user: String,
    pub deployment_ssh_key: String,
    pub ssh_agent_socket: String,
    pub debug: bool,
    pub done_tx: Sender<()>,
}

pub struct Api {
    config: ApiConfig,
    save_mu: Mutex<()>,
}

impl Api {
    pub fn new(config: ApiConfig) -> Self {
        Self {
            config,
            save_mu: Mutex::new(()),
        }
    }

    fn handle(self: Arc<Self>, request: tiny_http::Request) {
        let url = request.url().to_string();
        let method = request.method().clone();
        let path = url.split('?').next().unwrap_or("");
        match (method, path) {
            (Method::Get, "/ping") => self.ping(request),
            (Method::Post, "/install") => self.install(request),
            _ => {
                let _ = request.respond(Response::empty(404));
            }
        }
    }

    fn ping(self: &Arc<Self>, request: tiny_http::Request) {
        info!(ip = %client_ip(&request), "received ping from agent");
        let _ = request.respond(Response::from_string("pong"));
    }

    fn install(self: &Arc<Self>, mut request: tiny_http::Request) {
        let mut body = String::new();
        if let Err(error) = std::io::Read::read_to_string(&mut request.as_reader(), &mut body) {
            let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
            return;
        }
        let install_request: InstallRequest = match serde_json::from_str(&body) {
            Ok(value) => value,
            Err(error) => {
                let _ =
                    request.respond(Response::from_string(error.to_string()).with_status_code(400));
                return;
            }
        };

        let ip = client_ip(&request);
        let _span = info_span!(
            "api.install_request",
            "net.peer.ip" = %ip,
            "host.mac" = %install_request.mac_address,
        )
        .entered();
        info!(%ip, mac = %install_request.mac_address, "received install request from agent");

        let flake_output = match hosts::get_flake_output_by_mac(
            &install_request.mac_address,
            &self.config.hosts_config,
        ) {
            Ok(value) => value,
            Err(error) => {
                error!(%error, "failed to get flake by MAC address");
                let _ =
                    request.respond(Response::from_string(error.to_string()).with_status_code(404));
                return;
            }
        };
        let flake = format!("{}#{}", self.config.flake, flake_output);
        let Some(host) = self.config.hosts_config.get(&flake_output).cloned() else {
            let _ = request.respond(Response::empty(404));
            return;
        };

        if host.state() != State::Unknown {
            let message = "installation already in progress";
            let _ = request.respond(Response::from_string(message).with_status_code(409));
            return;
        }
        host.set_state(State::Installing);

        let api = Arc::clone(self);
        let flake_output_for_task = flake_output.clone();
        let parent = tracing::Span::current();
        std::thread::spawn(move || {
            let _guard = parent.enter();
            if let Err(error) = api.install_host(&host, &flake_output_for_task, &flake, &ip) {
                error!(%error, %ip, "failed to install host");
                host.set_state(State::Failed);
            }
            if hosts::all_installed(&api.config.hosts_config) {
                debug!("all hosts installed, signaling completion");
                let _ = api.config.done_tx.send(());
            }
        });

        let _ = request.respond(
            Response::from_string("installation started")
                .with_status_code(202)
                .with_header(Header::from_bytes("Content-Type", "text/plain").unwrap()),
        );
    }

    fn install_host(
        &self,
        host: &Arc<Host>,
        flake_output: &str,
        flake: &str,
        ip: &str,
    ) -> Result<()> {
        let span = info_span!(
            "api.install_host",
            "host.mac" = %host.mac_address(),
            "net.peer.ip" = ip,
            "nix.flake_output" = flake_output,
            "nix.flake_ref" = flake,
            "host.machine_id_hash" = tracing::field::Empty,
        );
        let _guard = span.enter();

        info!(%ip, flake_output, "installing NixOS");
        nixos::install(
            flake,
            "root",
            ip,
            &self.config.install_ssh_key,
            &self.config.ssh_agent_socket,
            self.config.debug,
        )
        .context("failed to install NixOS")?;

        let machine_id_hash = nixos::read_machine_id_hash(
            &self.config.deployment_ssh_user,
            ip,
            &self.config.deployment_ssh_key,
            &self.config.ssh_agent_socket,
            self.config.debug,
        )
        .context("failed to read final machine ID")?;

        span.record("host.machine_id_hash", machine_id_hash.as_str());
        host.set_final_identity(ip.to_string(), machine_id_hash);
        self.save_hosts().context("failed to save hosts config")?;
        host.set_state(State::Installed);
        info!(%ip, "successfully installed NixOS");
        Ok(())
    }

    fn save_hosts(&self) -> Result<()> {
        let _guard = self.save_mu.lock().unwrap();
        hosts::save_hosts_config(&self.config.hosts_file, &self.config.hosts_config)
    }
}

fn client_ip(request: &tiny_http::Request) -> String {
    request
        .remote_addr()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_default()
}

/// Serve the API on `0.0.0.0:5000` forever.
pub fn start_api_server(api: Api) -> Result<()> {
    let address = SocketAddr::from(([0, 0, 0, 0], API_PORT));
    let server =
        tiny_http::Server::http(address).map_err(|e| anyhow::anyhow!("API server: {e}"))?;
    info!(%address, "starting API server");

    let api = Arc::new(api);
    let parent = tracing::Span::current();
    for request in server.incoming_requests() {
        let api = Arc::clone(&api);
        let parent = parent.clone();
        std::thread::spawn(move || {
            let _guard = parent.enter();
            api.handle(request);
        });
    }

    warn!("API server stopped");
    Ok(())
}
