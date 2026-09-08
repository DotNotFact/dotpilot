//! Helpers for running PowerShell / console tools without flashing windows.
use anyhow::{anyhow, Result};
use base64::Engine;
use std::os::windows::process::CommandExt;
use std::process::Command;

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Number of PowerShell invocations since start (for the self-monitor).
pub static PS_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Run a PowerShell script (UTF-8 output) and return stdout.
pub fn run_ps(script: &str) -> Result<String> {
    PS_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let full = format!(
        "[Console]::OutputEncoding=[Text.Encoding]::UTF8; $ProgressPreference='SilentlyContinue'; $ErrorActionPreference='Stop'; {}",
        script
    );
    let utf16: Vec<u8> = full.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    let out = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let msg = if stderr.trim().is_empty() { stdout.clone() } else { stderr };
        return Err(anyhow!("{}", msg.trim()));
    }
    Ok(stdout)
}

/// Run a PowerShell script that ends with `ConvertTo-Json` and parse the result.
pub fn run_ps_json<T: serde::de::DeserializeOwned>(script: &str) -> Result<T> {
    let out = run_ps(script)?;
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return serde_json::from_str("[]").map_err(|e| anyhow!("empty PS output: {e}"));
    }
    serde_json::from_str(trimmed).map_err(|e| anyhow!("PS JSON parse error: {e}\n{trimmed}"))
}

/// Run a plain console program and capture stdout (UTF-8 or CP866).
pub fn run_cmd(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    let stdout = decode_console(&out.stdout);
    if !out.status.success() {
        let stderr = decode_console(&out.stderr);
        return Err(anyhow!(
            "{} {:?}: {}",
            program,
            args,
            if stderr.trim().is_empty() { stdout } else { stderr }
        ));
    }
    Ok(stdout)
}

/// Console programs on a Russian Windows emit CP866; try UTF-8 first, then fall back.
fn decode_console(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    cp866_to_string(bytes)
}

fn cp866_to_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x00..=0x7f => b as char,
            0x80..=0xaf => char::from_u32(0x410 + (b - 0x80) as u32).unwrap_or('?'),
            0xe0..=0xef => char::from_u32(0x440 + (b - 0xe0) as u32).unwrap_or('?'),
            0xf0 => '\u{0401}',
            0xf1 => '\u{0451}',
            _ => '?',
        })
        .collect()
}

/// Spawn a program detached (no waiting).
pub fn spawn_detached(program: &str, args: &[&str], cwd: Option<&str>) -> Result<u32> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let child = cmd.spawn()?;
    Ok(child.id())
}

/// Quote a string as a single-quoted PowerShell literal.
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
