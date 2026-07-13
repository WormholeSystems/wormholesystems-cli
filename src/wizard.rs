//! Interactive layer: prompts, preflight gates, orchestration. What to
//! write and run is decided in `plan`, executed via `exec`.

use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use console::style;
use inquire::{Confirm, CustomType, Password, PasswordDisplayMode, Select, Text};

use crate::exec::{RealRunner, Runner, run_steps};
use crate::plan::{self, Answers, Mode, Step, StepGroup};
use crate::state::ResumeState;
use crate::{docker, envfile, netinfo, secrets};

const REPO_URL: &str = "https://github.com/WormholeSystems/wormholesystems-containers.git";

pub fn run(dir: Option<PathBuf>) -> Result<()> {
    println!(
        "{}",
        style("Wormhole Systems — container setup wizard").bold()
    );
    println!();

    docker::doctor()?;
    println!();
    confirm_docker_running()?;

    let repo = locate_repo(dir)?;
    println!("Using repo at {}\n", style(repo.display()).green());

    if let Some(state) = ResumeState::load(&repo)? {
        let done = state.completed.join(", ");
        let resume = Confirm::new(&format!(
            "Found an interrupted setup (completed: {}). Resume it?",
            if done.is_empty() {
                "nothing yet"
            } else {
                &done
            }
        ))
        .with_default(true)
        .prompt()?;
        if resume {
            return resume_setup(&repo, state);
        }
        state.delete()?;
        println!("Starting fresh.\n");
    }

    let mode = match Select::new(
        "Which setup do you want?",
        vec![
            "Production (Traefik, automatic SSL certificates)",
            "Local test (no SSL, localhost)",
        ],
    )
    .with_help_message(
        "production needs a server with a public IP, open ports 80/443 and a domain; \
         local test runs entirely on this machine",
    )
    .prompt()?
    {
        s if s.starts_with("Production") => Mode::Production,
        _ => Mode::Local,
    };

    let answers = collect_answers(mode)?;

    let template_name = match mode {
        Mode::Production => ".env.production.example",
        Mode::Local => ".env.local.example",
    };
    let template = fs::read_to_string(repo.join(template_name))
        .with_context(|| format!("cannot read {template_name}"))?;
    for file in plan::build_files(mode, &answers, &template) {
        write_confirmed(&repo.join(file.rel_path), &file.content)?;
    }

    if mode == Mode::Local {
        check_force_https_guard(&repo);
    }

    let compose_files = plan::compose_files(mode, answers.app_port, answers.reverb_port);

    if !preflight_buildable(&repo) {
        print_summary(
            mode,
            &answers.app_domain,
            &answers.ws_domain,
            answers.app_port,
            answers.reverb_port,
            &compose_files,
        );
        return Ok(());
    }

    let start_prompt = match mode {
        Mode::Production => "Create the `web` network, build and start the stack now?",
        Mode::Local => "Build and start the local test stack now?",
    };
    if Confirm::new(start_prompt).with_default(true).prompt()? {
        let state = ResumeState::new(&repo, mode, &answers);
        state.save()?;
        execute_remaining(&repo, state)?;
    } else {
        print_summary(
            mode,
            &answers.app_domain,
            &answers.ws_domain,
            answers.app_port,
            answers.reverb_port,
            &compose_files,
        );
    }
    Ok(())
}

/// Refreshes the EVE static data of a running instance (SDE download,
/// migrations, SDE seed), per the upstream README's update sequence.
pub fn update(dir: Option<PathBuf>) -> Result<()> {
    let repo = dir.unwrap_or(std::env::current_dir()?);
    if !looks_like_repo(&repo) {
        bail!(
            "{} is not a wormholesystems-containers checkout — run `wsctl update` \
             inside one or pass --dir",
            repo.display()
        );
    }

    let env =
        fs::read_to_string(repo.join(".env")).context("no .env found — run `wsctl setup` first")?;
    let mode =
        plan::mode_from_env(&env).context(".env has no APP_ENV — run `wsctl setup` first")?;

    if !docker::daemon_running() {
        bail!("the docker daemon is not running — start Docker first");
    }

    let files = plan::compose_files(mode, 80, 8080);
    let compose_hint = std::iter::once("docker compose".to_string())
        .chain(files.iter().map(|f| format!("-f {f}")))
        .collect::<Vec<_>>()
        .join(" ");
    let running = docker::running_services(&repo, &files)?;
    for service in ["app", "mysql"] {
        if !running.iter().any(|s| s == service) {
            bail!(
                "service `{service}` is not running — start the stack with `{compose_hint} up -d` first"
            );
        }
    }

    println!(
        "Updating EVE static data ({} stack, this downloads ~500MB and takes a while)...\n",
        match mode {
            Mode::Production => "production",
            Mode::Local => "local test",
        }
    );
    let mut runner = RealRunner;
    for action in plan::update_actions(mode) {
        match action {
            plan::Action::Command { program, args } => runner.run(&repo, &program, &args)?,
            plan::Action::EnsureWebNetwork => {}
        }
    }
    println!(
        "\n{} EVE static data is up to date.",
        style("✓").green().bold()
    );
    Ok(())
}

/// Shows this machine's public IP and the DNS records the configured
/// instance needs, then verifies where the domains currently point.
pub fn dns(dir: Option<PathBuf>) -> Result<()> {
    let repo = dir.unwrap_or(std::env::current_dir()?);
    let env = fs::read_to_string(repo.join(".env")).ok();

    let domains = env.as_deref().and_then(|env| {
        let app = envfile::value(env, "APP_DOMAIN")?;
        let ws = envfile::value(env, "WS_DOMAIN")?;
        Some((plan::mode_from_env(env), app, ws))
    });

    match domains {
        Some((Some(plan::Mode::Local), _, _)) => {
            println!("This is a local test setup — it runs on localhost and needs no DNS records.");
        }
        Some((_, app_domain, ws_domain)) => {
            let public_ip = print_dns_records(&app_domain, &ws_domain);
            if let Some(ip) = public_ip {
                println!("\n{}", style("Current DNS status:").bold());
                if check_domains(&app_domain, &ws_domain, ip) {
                    println!(
                        "\n{} both domains point to this server.",
                        style("✓").green().bold()
                    );
                }
            }
        }
        None => {
            println!(
                "No configured instance found here (missing .env) — run `wsctl dns` inside \
                 a set-up wormholesystems-containers checkout or pass --dir.\n\
                 Generic guidance for a production instance:"
            );
            print_dns_records("<your domain>", "ws.<your domain>");
        }
    }
    Ok(())
}

/// Prints the public IP and the records to create; returns the IP so
/// callers can verify against it (None if detection failed).
fn print_dns_records(app_domain: &str, ws_domain: &str) -> Option<IpAddr> {
    let public_ip = netinfo::public_ip();
    match public_ip {
        Some(ip) => println!("Public IP of this machine: {}", style(ip).cyan().bold()),
        None => println!(
            "{} could not detect this machine's public IP (offline?) — \
             showing placeholders.",
            style("Note:").yellow().bold()
        ),
    }

    let record = match public_ip {
        Some(IpAddr::V6(_)) => "AAAA",
        _ => "A",
    };
    let target = public_ip
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "<public IP of this server>".to_string());
    println!(
        "\n{}",
        style("Create these DNS records at your domain provider:").bold()
    );
    for domain in [app_domain, ws_domain] {
        println!("  {record:<5} {domain:<28} →  {target}");
    }
    println!(
        "\nWhy: Traefik proves to Let's Encrypt that you own the domains via an\n\
         HTTP challenge. Both domains must resolve to this server and ports 80\n\
         and 443 must be reachable from the internet — otherwise no SSL\n\
         certificate is issued and the site stays unreachable."
    );
    public_ip
}

/// Prints one status line per domain; true when both point to `ip`.
fn check_domains(app_domain: &str, ws_domain: &str, ip: IpAddr) -> bool {
    let mut all_ok = true;
    for domain in [app_domain, ws_domain] {
        let resolved = netinfo::resolve(domain);
        if resolved.contains(&ip) {
            println!("  {} {domain} → {ip}", style("✓").green().bold());
        } else if resolved.is_empty() {
            println!(
                "  {} {domain} does not resolve yet",
                style("✗").red().bold()
            );
            all_ok = false;
        } else {
            let others = resolved
                .iter()
                .map(|ip| ip.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "  {} {domain} → {others} {}",
                style("✗").red().bold(),
                style("(not this server)").red()
            );
            all_ok = false;
        }
    }
    all_ok
}

/// Interactive wrapper around the DNS guidance: lets the user fix the
/// records and re-check, or continue while DNS still propagates.
fn dns_gate(app_domain: &str, ws_domain: &str) -> Result<()> {
    println!();
    let Some(ip) = print_dns_records(app_domain, ws_domain) else {
        return Ok(());
    };

    println!("\n{}", style("Checking where the domains point...").bold());
    loop {
        if check_domains(app_domain, ws_domain, ip) {
            println!(
                "{} both domains point to this server.\n",
                style("✓").green().bold()
            );
            return Ok(());
        }
        let choice = Select::new(
            "How do you want to proceed?",
            vec![
                "I have created/updated the records — check again",
                "Continue anyway (Traefik retries certificates once DNS is correct)",
            ],
        )
        .with_help_message("new DNS records can take a few minutes to propagate")
        .prompt()?;
        if choice.starts_with("Continue") {
            println!();
            return Ok(());
        }
        println!();
    }
}

/// The config files are already on disk; only the remaining steps run.
fn resume_setup(repo: &Path, state: ResumeState) -> Result<()> {
    if !preflight_buildable(repo) {
        return Ok(());
    }
    execute_remaining(repo, state)
}

fn execute_remaining(repo: &Path, mut state: ResumeState) -> Result<()> {
    let mode = state.mode;
    let steps = plan::build_steps(mode, state.app_port, state.reverb_port);
    let compose_files = plan::compose_files(mode, state.app_port, state.reverb_port);

    let group: Vec<&Step> = steps
        .iter()
        .filter(|s| s.group == StepGroup::Stack)
        .collect();
    let stack_pending = group.iter().any(|s| !state.is_done(s.id));

    if stack_pending {
        if !docker::daemon_running() {
            println!(
                "{} the docker daemon is not running.\n\
                 Start Docker and re-run `wsctl setup` — it will resume where it stopped.",
                style("Skipping:").yellow().bold()
            );
            print_summary(
                mode,
                &state.app_domain,
                &state.ws_domain,
                state.app_port,
                state.reverb_port,
                &compose_files,
            );
            return Ok(());
        }
        // After `up` succeeds the stack itself legitimately holds the ports.
        if !state.is_done("up")
            && !confirm_ports_free(&plan::stack_ports(mode, state.app_port, state.reverb_port))?
        {
            print_summary(
                mode,
                &state.app_domain,
                &state.ws_domain,
                state.app_port,
                state.reverb_port,
                &compose_files,
            );
            return Ok(());
        }
        run_steps(&mut RealRunner, repo, &group, &mut state)?;
    }

    let init: Vec<&Step> = steps
        .iter()
        .filter(|s| s.group == StepGroup::Init)
        .collect();
    let init_pending = init.iter().any(|s| !state.is_done(s.id));
    if init_pending
        && Confirm::new(
            "Initialize the application now? (downloads ~500MB of EVE SDE data, runs migrations — takes a while)",
        )
        .with_default(true)
        .prompt()?
    {
        run_steps(&mut RealRunner, repo, &init, &mut state)?;
    }

    let all_done = steps.iter().all(|s| state.is_done(s.id));
    print_summary(
        mode,
        &state.app_domain,
        &state.ws_domain,
        state.app_port,
        state.reverb_port,
        &compose_files,
    );
    if all_done {
        state.delete()?;
    } else {
        println!(
            "{} initialization is still pending — re-run `wsctl setup` to resume it.",
            style("Note:").yellow().bold()
        );
    }
    Ok(())
}

/// A partial clone (e.g. uninitialized wormhole-systems submodule) would
/// otherwise fail with a cryptic error deep inside the docker build.
fn preflight_buildable(repo: &Path) -> bool {
    let missing: Vec<&str> = [
        "dockerfiles/common/frankenPHP/Dockerfile",
        "wormhole-systems/composer.json",
    ]
    .into_iter()
    .filter(|p| !repo.join(p).is_file())
    .collect();
    if missing.is_empty() {
        return true;
    }
    println!(
        "{} this checkout cannot build the stack — missing:\n{}\n\
         Run `git submodule update --init` in the repo (or clone it with\n\
         --recurse-submodules), then re-run `wsctl setup`. Skipping the build.",
        style("Warning:").yellow().bold(),
        missing
            .iter()
            .map(|m| format!("  - {m}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    false
}

/// Returns false only when the user chooses to continue without Docker.
fn confirm_docker_running() -> Result<bool> {
    if docker::daemon_running() {
        return Ok(true);
    }

    let line = "─".repeat(64);
    println!("{}", style(&line).red());
    println!("{}", style("  ⚠  DOCKER IS NOT RUNNING").red().bold());
    println!(
        "{}",
        style("     Please start Docker first before continuing with the setup.").red()
    );
    println!("{}\n", style(&line).red());

    loop {
        let choice = Select::new(
            "How do you want to proceed?",
            vec![
                "I have started Docker — check again",
                "Continue without Docker (only generate the config files)",
                "Abort setup",
            ],
        )
        .prompt()?;

        if choice.starts_with("Continue") {
            return Ok(false);
        }
        if choice.starts_with("Abort") {
            bail!("setup aborted — start Docker and run `wsctl setup` again");
        }

        // Docker Desktop's API needs a few seconds after launch.
        print!("Checking");
        for _ in 0..5 {
            if docker::daemon_running() {
                println!("\n{} Docker is running.\n", style("✓").green().bold());
                return Ok(true);
            }
            print!(".");
            use std::io::Write;
            std::io::stdout().flush().ok();
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        println!(
            "\n{} Docker is still not running. Make sure Docker (Desktop) has\n\
             finished starting — the whale icon stops animating — then try again.\n",
            style("✗").red().bold()
        );
    }
}

fn busy_ports(ports: &[u16]) -> Vec<u16> {
    use std::net::{SocketAddr, TcpStream};
    ports
        .iter()
        .copied()
        .filter(|&port| {
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(250)).is_ok()
        })
        .collect()
}

fn port_holder(port: u16) -> Option<String> {
    let out = std::process::Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().nth(1)?;
    let mut cols = line.split_whitespace();
    let command = cols.next()?;
    let pid = cols.next()?;
    Some(format!("{command} (pid {pid})"))
}

/// Returns false only when the user chooses to skip the build.
fn confirm_ports_free(ports: &[u16]) -> Result<bool> {
    let mut first = true;
    loop {
        let busy = busy_ports(ports);
        if busy.is_empty() {
            if !first {
                println!("{} all ports are free.\n", style("✓").green().bold());
            }
            return Ok(true);
        }

        let line = "─".repeat(64);
        println!("{}", style(&line).red());
        println!("{}", style("  ⚠  REQUIRED PORTS ARE IN USE").red().bold());
        for port in &busy {
            let holder = port_holder(*port)
                .map(|h| format!(" — used by {h}"))
                .unwrap_or_default();
            println!("{}", style(format!("     port {port}{holder}")).red());
        }
        println!(
            "{}",
            style("     Stop these services so the stack can bind its ports.").red()
        );
        println!("{}\n", style(&line).red());
        first = false;

        let choice = Select::new(
            "How do you want to proceed?",
            vec![
                "I have freed the ports — check again",
                "Skip the build (the config files are already written)",
                "Abort setup",
            ],
        )
        .prompt()?;
        if choice.starts_with("Skip") {
            return Ok(false);
        }
        if choice.starts_with("Abort") {
            bail!("setup aborted — free the ports and run `wsctl setup` again");
        }
    }
}

fn locate_repo(dir: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = dir {
        if !looks_like_repo(&dir) {
            bail!(
                "{} does not look like a wormholesystems-containers checkout \
                 (docker-compose.yml / .env.production.example missing)",
                dir.display()
            );
        }
        return Ok(dir);
    }

    let cwd = std::env::current_dir()?;
    if looks_like_repo(&cwd) {
        return Ok(cwd);
    }

    println!("The current directory is not a wormholesystems-containers checkout.");
    let parent = Text::new("Where should I clone it? (parent directory)")
        .with_default(&cwd.to_string_lossy())
        .with_help_message("the repo will be cloned to <directory>/wormholesystems-containers")
        .prompt()?;
    let parent = PathBuf::from(shellexpand_home(&parent));
    let target = parent.join("wormholesystems-containers");

    if looks_like_repo(&target) {
        println!("Found an existing checkout at {}.", target.display());
        return Ok(target);
    }
    if target.exists() {
        bail!("{} exists but is not the repo", target.display());
    }

    fs::create_dir_all(&parent).with_context(|| format!("cannot create {}", parent.display()))?;
    docker::run(&parent, "git", &["clone", "--recurse-submodules", REPO_URL])?;
    Ok(target)
}

fn looks_like_repo(dir: &Path) -> bool {
    dir.join("docker-compose.yml").is_file() && dir.join(".env.production.example").is_file()
}

fn shellexpand_home(path: &str) -> String {
    match (path.strip_prefix("~"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}{rest}"),
        _ => path.to_string(),
    }
}

fn collect_answers(mode: Mode) -> Result<Answers> {
    // Ports first: the EVE callback URL shown below depends on the app port.
    let (app_port, reverb_port) = match mode {
        Mode::Production => (80, 8080),
        Mode::Local => (
            pick_port("app", 80, 8000)?,
            pick_port("Reverb websocket server", 8080, 8081)?,
        ),
    };

    let (app_domain, ws_domain, acme_email) = match mode {
        Mode::Production => {
            let app_domain = Text::new("Main domain (e.g. wormhole.systems):")
                .with_validator(required)
                .with_help_message("the address your users will open in the browser")
                .prompt()?;
            let ws_domain = Text::new("WebSocket domain:")
                .with_default(&format!("ws.{app_domain}"))
                .with_help_message(
                    "live map updates flow over a separate websocket server (Reverb), \
                     which Traefik routes via its own subdomain",
                )
                .prompt()?;
            let acme_email = Text::new("Email for Let's Encrypt certificate notifications:")
                .with_validator(required)
                .with_help_message(
                    "Let's Encrypt sends certificate expiry and problem notices here — \
                     no account signup needed",
                )
                .prompt()?;
            dns_gate(&app_domain, &ws_domain)?;
            (app_domain, ws_domain, acme_email)
        }
        Mode::Local => (
            "localhost".into(),
            format!("localhost:{reverb_port}"),
            "admin@localhost".into(),
        ),
    };

    println!(
        "\n{} CCP requires contact info on all third-party apps; a missing/invalid\n\
         CONTACT_EMAIL risks a ban from the EVE Online API.",
        style("Important:").yellow().bold()
    );
    let contact_mail = Text::new("Contact email:")
        .with_validator(required)
        .with_help_message("sent to CCP with every EVE API request so they can reach you")
        .prompt()?;
    let contact_name = Text::new("Your EVE character name:")
        .with_validator(required)
        .prompt()?;
    let contact_email = format!("{contact_mail} | {contact_name}");

    let scope_list = plan::EsiScope::DEFAULTS
        .iter()
        .map(|s| {
            format!(
                "      {:<32} {}",
                s.as_str(),
                style(format!("— {}", s.purpose())).dim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    println!(
        "\nCreate an application at {} and paste its credentials.\n\
         Configure it as follows:\n  \
         - Application type: {}\n  \
         - Callback URL:     {}/eve/callback\n  \
         - Scopes (select all {}):\n{scope_list}",
        style("https://developers.eveonline.com/").cyan(),
        style("Authentication & API Access").cyan(),
        style(match mode {
            Mode::Production => format!("https://{app_domain}"),
            Mode::Local => plan::local_app_url(app_port),
        })
        .cyan(),
        plan::EsiScope::DEFAULTS.len(),
    );
    let eve_client_id = Text::new("EVE Client ID:")
        .with_validator(required)
        .prompt()?;
    let eve_client_secret = Password::new("EVE Client Secret:")
        .with_display_mode(PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()?;

    let allowed_affiliations = Text::new(
        "Restrict logins to these EVE character/corp/alliance IDs (comma separated, empty = anyone):",
    )
    .with_default("")
    .with_help_message(
        "keeps your instance private to your corp/alliance — find IDs in zkillboard.com URLs",
    )
    .prompt()?;

    println!();
    let db_database = Text::new("Database name:")
        .with_default("wormholesystems")
        .prompt()?;
    let db_username = Text::new("Database user:")
        .with_default("wormholesystems")
        .prompt()?;
    let (db_password, db_root_password) = if Confirm::new("Generate the database passwords for me?")
        .with_default(true)
        .prompt()?
    {
        (secrets::alphanumeric(24), secrets::alphanumeric(32))
    } else {
        let user = Password::new("Database password:")
            .with_display_mode(PasswordDisplayMode::Masked)
            .prompt()?;
        let root = Password::new("MySQL root password:")
            .with_display_mode(PasswordDisplayMode::Masked)
            .prompt()?;
        (user, root)
    };

    Ok(Answers {
        app_port,
        reverb_port,
        app_domain,
        ws_domain,
        acme_email,
        contact_email,
        eve_client_id,
        eve_client_secret,
        allowed_affiliations,
        db_database,
        db_username,
        db_password,
        db_root_password,
    })
}

/// Uses `preferred` if free, otherwise asks for a custom port.
fn pick_port(label: &str, preferred: u16, fallback: u16) -> Result<u16> {
    if busy_ports(&[preferred]).is_empty() {
        return Ok(preferred);
    }
    let holder = port_holder(preferred)
        .map(|h| format!(" by {h}"))
        .unwrap_or_default();
    println!(
        "{} port {preferred} is already in use{holder}.",
        style("Note:").yellow().bold()
    );
    loop {
        let port =
            CustomType::<u16>::new(&format!("Which host port should the {label} use instead?"))
                .with_default(fallback)
                .prompt()?;
        if port != preferred && busy_ports(&[port]).is_empty() {
            return Ok(port);
        }
        let holder = port_holder(port)
            .map(|h| format!(" by {h}"))
            .unwrap_or_default();
        println!("Port {port} is also in use{holder} — pick another.");
    }
}

fn required(input: &str) -> Result<inquire::validator::Validation, inquire::CustomUserError> {
    use inquire::validator::Validation;
    Ok(if input.trim().is_empty() {
        Validation::Invalid("a value is required".into())
    } else {
        Validation::Valid
    })
}

/// Asks before overwriting an existing file.
fn write_confirmed(path: &Path, content: &str) -> Result<()> {
    if path.exists()
        && !Confirm::new(&format!("{} already exists. Overwrite?", path.display()))
            .with_default(false)
            .prompt()?
    {
        println!("Keeping existing {}.", path.display());
        return Ok(());
    }
    fs::write(path, content).with_context(|| format!("cannot write {}", path.display()))?;
    println!("Wrote {}", style(path.display()).green());
    Ok(())
}

/// Old app revisions called `URL::forceHttps()` unconditionally, which
/// breaks plain-HTTP local setups; warn only when a checkout needs the edit.
fn check_force_https_guard(repo: &Path) {
    let provider = repo.join("wormhole-systems/app/Providers/AppServiceProvider.php");
    let Ok(source) = fs::read_to_string(&provider) else {
        println!(
            "{} {} not found — did you clone with --recurse-submodules?",
            style("Warning:").yellow().bold(),
            provider.display()
        );
        return;
    };
    if source.contains("URL::forceHttps") && !source.contains("environment(['local'") {
        println!(
            "{} this app revision calls URL::forceHttps() unconditionally, which\n\
             breaks plain-HTTP local access. Comment out that line in\n\
             {} or update the submodule.",
            style("Warning:").yellow().bold(),
            provider.display()
        );
    }
}

fn print_summary(
    mode: Mode,
    app_domain: &str,
    ws_domain: &str,
    app_port: u16,
    reverb_port: u16,
    compose_files: &[String],
) {
    println!("\n{}", style("Setup complete!").green().bold());
    match mode {
        Mode::Production => {
            println!("  App:       https://{app_domain}\n  WebSocket: wss://{ws_domain}")
        }
        Mode::Local => println!(
            "  App:       {}\n  WebSocket: ws://localhost:{reverb_port}",
            plan::local_app_url(app_port),
        ),
    }
    let compose = std::iter::once("docker compose".to_string())
        .chain(compose_files.iter().map(|f| format!("-f {f}")))
        .collect::<Vec<_>>()
        .join(" ");
    println!(
        "\nCredentials are stored in .env and dockerfiles/mysql/.env — keep them safe.\n\
         First login: use EVE Online SSO with your EVE character.\n\
         Check services with `{compose} ps`, logs with `{compose} logs -f`."
    );
}
