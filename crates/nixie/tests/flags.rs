//! Command-line flag parsing and SSH authentication validation.

use std::sync::Mutex;

use nixie::flags::Flags;

static ENV_LOCK: Mutex<()> = Mutex::new(());

const USAGE_ERROR: &str = "missing flags, usage: nixie --hosts <hosts.json> --flake <flake> \
--installer <installer-output> [--ssh-agent-socket <socket> | --install-ssh-key <private-key> \
--deployment-ssh-key <private-key>]";
const SSH_ERROR: &str = "SSH authentication requires SSH_AUTH_SOCK, --ssh-agent-socket, or both \
--install-ssh-key and --deployment-ssh-key";

fn parse(args: &[&str], environment_socket: &str) -> anyhow::Result<Flags> {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("SSH_AUTH_SOCK", environment_socket);
    let mut argv: Vec<String> = vec!["nixie".to_string()];
    argv.extend(args.iter().map(|arg| arg.to_string()));
    Flags::parse_args_from(argv)
}

fn base_args() -> Vec<String> {
    vec![
        "--hosts".to_string(),
        "hosts.json".to_string(),
        "--flake".to_string(),
        ".".to_string(),
        "--installer".to_string(),
        ".#nixosConfigurations.installer".to_string(),
    ]
}

#[test]
fn validates_ssh_authentication() {
    struct Case {
        name: &'static str,
        install_key: &'static str,
        deployment_key: &'static str,
        environment_socket: &'static str,
        agent_socket: &'static str,
        want_socket: &'static str,
        accepted: bool,
    }
    let cases = [
        Case {
            name: "missing authentication",
            install_key: "",
            deployment_key: "",
            environment_socket: "",
            agent_socket: "",
            want_socket: "",
            accepted: false,
        },
        Case {
            name: "installer key only",
            install_key: "/keys/installer",
            deployment_key: "",
            environment_socket: "",
            agent_socket: "",
            want_socket: "",
            accepted: false,
        },
        Case {
            name: "deployment key only",
            install_key: "",
            deployment_key: "/keys/deployment",
            environment_socket: "",
            agent_socket: "",
            want_socket: "",
            accepted: false,
        },
        Case {
            name: "both keys",
            install_key: "/keys/installer",
            deployment_key: "/keys/deployment",
            environment_socket: "",
            agent_socket: "",
            want_socket: "",
            accepted: true,
        },
        Case {
            name: "environment agent",
            install_key: "",
            deployment_key: "",
            environment_socket: "/run/user/1000/environment-agent.sock",
            agent_socket: "",
            want_socket: "/run/user/1000/environment-agent.sock",
            accepted: true,
        },
        Case {
            name: "agent only",
            install_key: "",
            deployment_key: "",
            environment_socket: "",
            agent_socket: "/run/user/1000/ssh-agent.sock",
            want_socket: "/run/user/1000/ssh-agent.sock",
            accepted: true,
        },
        Case {
            name: "explicit agent overrides environment",
            install_key: "",
            deployment_key: "",
            environment_socket: "/run/user/1000/environment-agent.sock",
            agent_socket: "/run/user/1000/explicit-agent.sock",
            want_socket: "/run/user/1000/explicit-agent.sock",
            accepted: true,
        },
        Case {
            name: "agent and installer key",
            install_key: "/keys/installer",
            deployment_key: "",
            environment_socket: "",
            agent_socket: "/run/user/1000/ssh-agent.sock",
            want_socket: "/run/user/1000/ssh-agent.sock",
            accepted: true,
        },
        Case {
            name: "agent and deployment key",
            install_key: "",
            deployment_key: "/keys/deployment",
            environment_socket: "",
            agent_socket: "/run/user/1000/ssh-agent.sock",
            want_socket: "/run/user/1000/ssh-agent.sock",
            accepted: true,
        },
        Case {
            name: "agent and both keys",
            install_key: "/keys/installer",
            deployment_key: "/keys/deployment",
            environment_socket: "",
            agent_socket: "/run/user/1000/ssh-agent.sock",
            want_socket: "/run/user/1000/ssh-agent.sock",
            accepted: true,
        },
    ];

    for case in cases {
        let mut args = base_args();
        if !case.install_key.is_empty() {
            args.push("--install-ssh-key".into());
            args.push(case.install_key.into());
        }
        if !case.deployment_key.is_empty() {
            args.push("--deployment-ssh-key".into());
            args.push(case.deployment_key.into());
        }
        if !case.agent_socket.is_empty() {
            args.push("--ssh-agent-socket".into());
            args.push(case.agent_socket.into());
        }

        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let result = parse(&borrowed, case.environment_socket);

        if !case.accepted {
            let error = result.expect_err(case.name);
            assert_eq!(error.to_string(), SSH_ERROR, "case {}", case.name);
            continue;
        }

        let flags = result.unwrap_or_else(|e| panic!("case {}: {e}", case.name));
        assert_eq!(flags.hosts, "hosts.json", "case {}", case.name);
        assert_eq!(flags.flake, ".", "case {}", case.name);
        assert_eq!(
            flags.installer, ".#nixosConfigurations.installer",
            "case {}",
            case.name
        );
        assert_eq!(flags.deployment_ssh_user, "root", "case {}", case.name);
        assert_eq!(
            flags.install_ssh_key, case.install_key,
            "case {}",
            case.name
        );
        assert_eq!(
            flags.deployment_ssh_key, case.deployment_key,
            "case {}",
            case.name
        );
        assert_eq!(
            flags.ssh_agent_socket, case.want_socket,
            "case {}",
            case.name
        );
    }
}

#[test]
fn requires_deployment_inputs() {
    let cases = [
        (
            "missing hosts file",
            "",
            ".",
            ".#nixosConfigurations.installer",
        ),
        (
            "missing flake",
            "hosts.json",
            "",
            ".#nixosConfigurations.installer",
        ),
        ("missing installer", "hosts.json", ".", ""),
    ];

    for (name, hosts, flake, installer) in cases {
        let mut args: Vec<String> = vec![
            "--ssh-agent-socket".into(),
            "/run/user/1000/ssh-agent.sock".into(),
        ];
        if !hosts.is_empty() {
            args.push("--hosts".into());
            args.push(hosts.into());
        }
        if !flake.is_empty() {
            args.push("--flake".into());
            args.push(flake.into());
        }
        if !installer.is_empty() {
            args.push("--installer".into());
            args.push(installer.into());
        }

        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let error = parse(&borrowed, "").expect_err(name);
        assert_eq!(error.to_string(), USAGE_ERROR, "case {name}");
    }
}
