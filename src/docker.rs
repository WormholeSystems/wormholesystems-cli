use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use console::style;

pub fn run(dir: &Path, program: &str, args: &[&str]) -> Result<()> {
    println!(
        "{} {} {}",
        style("$").dim(),
        style(program).cyan(),
        style(args.join(" ")).cyan()
    );
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .with_context(|| format!("failed to start `{program}` — is it installed?"))?;
    if !status.success() {
        bail!("`{program} {}` exited with {status}", args.join(" "));
    }
    Ok(())
}

/// Whether the docker daemon is reachable (`docker --version` succeeds
/// even when the daemon is down, so this probes the API).
pub fn daemon_running() -> bool {
    Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The external network Traefik and the app share in production.
pub fn ensure_web_network(dir: &Path) -> Result<()> {
    let exists = Command::new("docker")
        .args(["network", "inspect", "web"])
        .current_dir(dir)
        .output()
        .context("failed to run docker")?
        .status
        .success();
    if exists {
        println!("Network {} already exists, skipping.", style("web").green());
        return Ok(());
    }
    run(dir, "docker", &["network", "create", "-d", "bridge", "web"])
}

/// Missing tools are an error; a stopped daemon is not (config files can
/// still be generated without it), so that is returned as a bool.
pub fn doctor() -> Result<bool> {
    let mut ok = true;
    for (label, program, args) in [
        ("git", "git", &["--version"][..]),
        ("docker", "docker", &["--version"]),
        ("docker compose", "docker", &["compose", "version"]),
    ] {
        let found = Command::new(program)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        let mark = if found {
            style("ok").green()
        } else {
            ok = false;
            style("missing").red()
        };
        println!("{label:>16}  {mark}");
    }
    if !ok {
        bail!("some required tools are missing");
    }

    let daemon = daemon_running();
    let mark = if daemon {
        style("running").green()
    } else {
        style("not running").red()
    };
    println!("{:>16}  {mark}", "docker daemon");
    Ok(daemon)
}
