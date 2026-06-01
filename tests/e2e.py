#!/usr/bin/env python3

import json
import os
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ed25519


BRIDGE = "brnxe"
CONTROLLER_IP = "192.168.50.2"
CIDR = "24"
NIXIE_API_PORT = 5000
OTEL_ENDPOINT = "127.0.0.1:14317"
TESTS_DIR = Path("tests")

TIMEOUT = {"api": 30 * 60, "install": 45 * 60, "ssh": 30 * 60, "cold": 15 * 60}
REQUIRED_ENV = ("NIXIE_BIN", "OVMF_CODE", "OVMF_VARS")
FAILURES = (
    "failed to install NixOS",
    "failed to read final machine ID",
    "failed to save hosts config",
)
SSH_SEED = bytes.fromhex("62d7724c580bc35680ef58daa05ddbdf8ce37f3de511d246973f64c227c8d8e7")
SSH_OPTS = (
    "-o", "BatchMode=yes", "-o", "ConnectTimeout=2",
    "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
)

PROCESSES = []
LINKS = []


def log(message):
    print(f"[nixie-e2e] {message}", flush=True)


def run(cmd, **kwargs):
    log(f"+ {' '.join(shlex.quote(part) for part in cmd)}")
    subprocess.run(cmd, check=True, **kwargs)


def read(path):
    if not path.exists():
        return ""
    return path.read_text(encoding="utf-8", errors="replace")


def print_tail(label, path, stream=sys.stdout):
    text = "\n".join(read(path).splitlines()[-40:])
    if text:
        print(f"\n== {label} ==\n{text}", file=stream)


def start(name, cmd, log_path, env=None, cwd=None):
    log(f"starting {name}: {' '.join(shlex.quote(part) for part in cmd)}")
    handle = open(log_path, "w", encoding="utf-8")
    process = subprocess.Popen(
        cmd, stdout=handle, stderr=subprocess.STDOUT,
        start_new_session=True, text=True, env=env, cwd=cwd,
    )
    process.name = name
    process.log_path = log_path
    process.log_handle = handle
    PROCESSES.append(process)
    return process


def stop(process):
    if process in PROCESSES:
        PROCESSES.remove(process)
    try:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=10)
    finally:
        process.log_handle.close()


def cleanup():
    for process in reversed(PROCESSES):
        stop(process)
    for link in reversed(LINKS):
        subprocess.run(["ip", "link", "del", link], check=False)


def wait_tcp(host, port, timeout, process=None):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if process and process.poll() is not None:
            output = read(process.log_path)[-4000:]
            raise RuntimeError(f"{process.name} exited before {host}:{port} was ready\n{output}")
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(1)
            try:
                sock.connect((host, port))
                return
            except OSError:
                time.sleep(1)
    raise TimeoutError(f"timed out waiting for {host}:{port}")


def wait_nixie(process):
    deadline = time.time() + TIMEOUT["install"]
    while time.time() < deadline:
        rc = process.poll()
        if rc is not None:
            if rc != 0:
                raise RuntimeError(f"nixie exited with status {rc}")
            return

        output = read(process.log_path)[-200000:]
        for marker in FAILURES:
            if marker in output:
                raise RuntimeError(f"nixie reported failure: {marker}")
        time.sleep(1)
    raise TimeoutError("timed out waiting for nixie to finish installing all hosts")


def wait_ssh(key_path, machine, timeout):
    cmd = ["ssh", "-i", str(key_path), *SSH_OPTS, f"root@{machine['ip']}", "hostname"]
    deadline = time.time() + timeout
    while time.time() < deadline:
        result = subprocess.run(cmd, capture_output=True, text=True)
        if result.returncode == 0 and result.stdout.strip() == machine["name"]:
            return
        time.sleep(2)
    raise TimeoutError(f"timed out waiting for SSH on {machine['ip']}")


def write_key(workdir):
    key_path = workdir / "id_ed25519"
    key = ed25519.Ed25519PrivateKey.from_private_bytes(SSH_SEED)
    key_path.write_bytes(
        key.private_bytes(
            encoding=serialization.Encoding.PEM,
            format=serialization.PrivateFormat.OpenSSH,
            encryption_algorithm=serialization.NoEncryption(),
        )
    )
    key_path.chmod(0o600)
    return key_path


def load_machines(hosts_path):
    machines = []
    for index, (name, host) in enumerate(json.loads(hosts_path.read_text()).items(), start=1):
        mac = host.get("mac_address")
        if not mac:
            raise RuntimeError(f"missing MAC address for {name} in {hosts_path}")
        machines.append({"name": name, "mac": mac, "ip": f"192.168.50.{10 + index}"})
    if not machines:
        raise RuntimeError(f"no hosts defined in {hosts_path}")
    return machines


def check_hosts(hosts_path, machines):
    hosts = json.loads(hosts_path.read_text())
    for machine in machines:
        host = hosts[machine["name"]]
        if host.get("ip") != machine["ip"] or len(host.get("machine_id_hash", "")) != 64:
            raise RuntimeError(f"unexpected generated host data for {machine['name']}")


def start_vm(machine, tap, workdir, initialize_disk):
    name = machine["name"]
    disk = workdir / f"{name}.qcow2"
    vars_path = workdir / f"{name}-OVMF_VARS.fd"
    if initialize_disk:
        shutil.copyfile(os.environ["OVMF_VARS"], vars_path)
        run(["qemu-img", "create", "-f", "qcow2", str(disk), "20G"])

    cmd = [
        "qemu-system-x86_64", "-name", name, "-machine", "q35",
        "-m", "2048", "-smp", "2", "-display", "none",
        "-serial", f"file:{workdir / f'{name}.serial.log'}",
        "-monitor", "none",
        "-drive", f"if=pflash,format=raw,readonly=on,file={os.environ['OVMF_CODE']}",
        "-drive", f"if=pflash,format=raw,file={vars_path}",
        "-drive", f"if=none,id=disk0,file={disk},format=qcow2",
        "-device", "virtio-scsi-pci,id=scsi0", "-device", "scsi-hd,drive=disk0,bootindex=1",
        "-netdev", f"tap,id=net0,ifname={tap},script=no,downscript=no",
        "-device", f"e1000,netdev=net0,mac={machine['mac']},bootindex=2", "-enable-kvm", "-cpu", "host",
    ]
    return start(name, cmd, workdir / f"{name}.qemu.log")


def configure_network(taps):
    run(["ip", "link", "add", BRIDGE, "type", "bridge"])
    LINKS.append(BRIDGE)
    run(["ip", "addr", "add", f"{CONTROLLER_IP}/{CIDR}", "dev", BRIDGE])
    run(["ip", "link", "set", BRIDGE, "up"])

    for tap in taps:
        run(["ip", "tuntap", "add", "dev", tap, "mode", "tap", "user", "root"])
        LINKS.append(tap)
        run(["ip", "link", "set", tap, "master", BRIDGE])
        run(["ip", "link", "set", tap, "up"])


def start_services(key_path, flake_ref, hosts_path, workdir):
    start("otelcol", ["otelcol", "--config", str(TESTS_DIR / "otelcol.yaml")], workdir / "otelcol.log", cwd=workdir)
    wait_tcp("127.0.0.1", int(OTEL_ENDPOINT.rsplit(":", 1)[1]), 30)
    start("dnsmasq", ["dnsmasq", "--keep-in-foreground", f"--conf-file={TESTS_DIR / 'dnsmasq.conf'}"], workdir / "dnsmasq.log")

    env = os.environ | {
        "OTEL_EXPORTER_OTLP_ENDPOINT": f"http://{OTEL_ENDPOINT}",
        "OTEL_SERVICE_NAME": "nixie",
    }
    cmd = [
        os.environ["NIXIE_BIN"],
        "--address", CONTROLLER_IP,
        "--installer", f"{flake_ref}#nixosConfigurations.installer",
        "--flake", flake_ref, "--hosts", str(hosts_path),
        "--install-ssh-key", str(key_path), "--deployment-ssh-key", str(key_path), "--debug",
    ]
    return start("nixie", cmd, workdir / "nixie.log", env=env)


def verify_ssh(machines, key_path, timeout, message):
    for machine in machines:
        wait_ssh(key_path, machine, timeout)
        log(f"{message} {machine['name']} at {machine['ip']}")


def check_prerequisites():
    missing = [name for name in REQUIRED_ENV if not os.environ.get(name)]
    if os.geteuid() != 0:
        print("Run this as root, for example: sudo nix run .#e2e", file=sys.stderr)
    elif missing:
        print(f"missing required environment variables: {', '.join(missing)}", file=sys.stderr)
    elif not os.path.exists("/dev/kvm"):
        print("/dev/kvm is required to run the e2e harness", file=sys.stderr)
    elif not os.access("/dev/kvm", os.R_OK | os.W_OK):
        print("/dev/kvm is present but not accessible; fix its permissions and retry", file=sys.stderr)
    else:
        log("using KVM acceleration")
        return True
    return False


def main():
    if not check_prerequisites():
        return 1

    workdir = Path(".")
    flake = Path("examples")
    hosts_path = flake / "hosts.json"
    key_path = write_key(workdir)
    machines = load_machines(hosts_path)
    taps = [f"tapn{index + 1}" for index in range(len(machines))]
    qemus = []
    passed = False

    log(f"work directory: {workdir}")
    try:
        log(f"start e2e.run host_count={len(machines)}")
        configure_network(taps)
        nixie = start_services(key_path, f"./{flake}", hosts_path, workdir)
        wait_tcp(CONTROLLER_IP, NIXIE_API_PORT, TIMEOUT["api"], process=nixie)
        time.sleep(2)

        qemus = [start_vm(machine, tap, workdir, True) for machine, tap in zip(machines, taps, strict=True)]
        wait_nixie(nixie)
        check_hosts(hosts_path, machines)
        verify_ssh(machines, key_path, TIMEOUT["ssh"], "verified")

        log("power cycling machines to verify disk boot without the installer")
        for qemu in reversed(qemus):
            stop(qemu)
        qemus = [start_vm(machine, tap, workdir, False) for machine, tap in zip(machines, taps, strict=True)]
        verify_ssh(machines, key_path, TIMEOUT["cold"], "verified cold boot for")

        passed = True
        log("end-to-end test passed")
    except Exception as err:
        log(f"end-to-end test failed: {err}")
    finally:
        cleanup()
        if not passed:
            for name in ("otelcol", "nixie", "dnsmasq"):
                print_tail(f"{name}.log", workdir / f"{name}.log", stream=sys.stderr)
        for machine in machines:
            print_tail(f"{machine['name']}.serial.log", workdir / f"{machine['name']}.serial.log")
            print_tail(f"{machine['name']}.qemu.log", workdir / f"{machine['name']}.qemu.log")
        log(f"artifacts kept in {workdir}")

    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
