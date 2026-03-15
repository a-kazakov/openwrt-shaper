pub mod counters;
pub mod devices;
pub mod firewall;
pub mod nftables;
pub mod tc;

use std::process::Command;

/// Run a command and return Ok(stdout) or Err with combined output.
pub fn run_cmd(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program} {}: {e}", args.join(" ")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!(
            "{program} {}: {}{}",
            args.join(" "),
            stdout.trim(),
            stderr.trim()
        ))
    }
}

/// Run a command, ignoring errors (best-effort cleanup).
pub fn run_cmd_ignore(program: &str, args: &[&str]) {
    let _ = Command::new(program).args(args).output();
}
