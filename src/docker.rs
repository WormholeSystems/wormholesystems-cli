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
pub fn ensure_network(dir: &Path, name: &str) -> Result<()> {
    if network_exists(name) {
        println!("Network {} already exists, skipping.", style(name).green());
        return Ok(());
    }
    run(dir, "docker", &["network", "create", "-d", "bridge", name])
}

pub fn network_exists(name: &str) -> bool {
    Command::new("docker")
        .args(["network", "inspect", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Names of the running containers attached to a network (empty when the
/// network is missing or unused).
pub fn network_containers(name: &str) -> Vec<String> {
    let Ok(out) = Command::new("docker")
        .args([
            "network",
            "inspect",
            name,
            "--format",
            "{{range .Containers}}{{.Name}}\n{{end}}",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

pub fn running_services(dir: &Path, files: &[String]) -> Result<Vec<String>> {
    let mut args = vec!["compose".to_string()];
    for file in files {
        args.push("-f".to_string());
        args.push(file.clone());
    }
    args.extend(["ps", "--status", "running", "--services"].map(String::from));
    let out = Command::new("docker")
        .args(&args)
        .current_dir(dir)
        .output()
        .context("failed to run docker")?;
    if !out.status.success() {
        bail!(
            "`docker compose ps` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
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
