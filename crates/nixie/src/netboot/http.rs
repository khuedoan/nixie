//! iPXE boot script construction and small URL/template helpers.

use anyhow::{bail, Context, Result};

use super::booter::{BootSpec, Machine};

/// Build the iPXE script served to a booting machine, mirroring Pixiecore's
/// `ipxeScript`.
pub fn ipxe_script(machine: Machine, spec: &BootSpec, server_host: &str) -> Result<Vec<u8>> {
    if !spec.ipxe_script.is_empty() {
        return Ok(spec.ipxe_script.clone().into_bytes());
    }
    if spec.kernel.is_empty() {
        bail!("spec is missing Kernel");
    }

    let mac = machine.mac.to_string();
    let file_url = |id: &str, kind: &str| {
        format!(
            "http://{server_host}/_/file?name={}&type={kind}&mac={}",
            query_escape(id),
            query_escape(&mac)
        )
    };

    let mut script = String::from("#!ipxe\n");
    script.push_str(&format!(
        "kernel --name kernel {}\n",
        file_url(&spec.kernel, "kernel")
    ));
    for (i, initrd) in spec.initrd.iter().enumerate() {
        script.push_str(&format!(
            "initrd --name initrd{i} {}\n",
            file_url(initrd, "initrd")
        ));
    }
    script.push_str(&format!(
        "imgfetch --name ready http://{server_host}/_/booting?mac={} ||\n",
        query_escape(&mac)
    ));
    script.push_str("imgfree ready ||\n");

    script.push_str("boot kernel ");
    for i in 0..spec.initrd.len() {
        script.push_str(&format!("initrd=initrd{i} "));
    }

    let cmdline = expand_cmdline(&spec.cmdline, |id| {
        format!("http://{server_host}/_/file?name={}", query_escape(id))
    })?;
    script.push_str(&cmdline);
    script.push('\n');

    Ok(script.into_bytes())
}

/// Expand a cmdline template, supporting only the `{{ ID "name" }}` function
/// used by Pixiecore.
pub fn expand_cmdline(template: &str, mut id_url: impl FnMut(&str) -> String) -> Result<String> {
    let mut result = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .context("unterminated template action in cmdline")?;
        let action = after[..end].trim();
        let argument = action
            .strip_prefix("ID")
            .map(str::trim)
            .with_context(|| format!("unknown template function in cmdline action {action:?}"))?;
        let name = argument.trim_matches(|c| c == '"' || c == '\'' || c == '`');
        result.push_str(&id_url(name));
        rest = &after[end + 2..];
    }
    result.push_str(rest);

    let cmdline = result.trim().to_string();
    if cmdline.contains('\n') {
        bail!("cmdline contains a newline");
    }
    Ok(cmdline)
}

/// Extract and percent-decode a query parameter from a request target.
pub fn query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    for pair in query.split('&') {
        let (name, value) = match pair.split_once('=') {
            Some((name, value)) => (name, value),
            None => (pair, ""),
        };
        if name == key {
            return Some(percent_decode(value));
        }
    }
    None
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode like Go's `url.QueryEscape`.
fn query_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
