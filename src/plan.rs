//! Pure planning layer: answers in, plain data out (files to write,
//! commands to run) — no prompts, no side effects, fully unit testable.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{envfile, secrets};

pub const PORTS_OVERRIDE_FILE: &str = "docker-compose.wsctl-ports.yml";

/// The app's default login scopes, kept in sync by hand with the PHP
/// `EsiScope` enum (variant names must match its case names — the drift
/// tests compare them against the pinned app source).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EsiScope {
    PublicData,
    ReadLocations,
    ReadShip,
    ReadOnlineStatus,
    WriteWaypoint,
}

impl EsiScope {
    pub const DEFAULTS: [EsiScope; 5] = [
        EsiScope::PublicData,
        EsiScope::ReadLocations,
        EsiScope::ReadShip,
        EsiScope::ReadOnlineStatus,
        EsiScope::WriteWaypoint,
    ];

    /// The scope name as listed in the EVE developer portal.
    pub fn as_str(self) -> &'static str {
        match self {
            EsiScope::PublicData => "publicData",
            EsiScope::ReadLocations => "esi-location.read_location.v1",
            EsiScope::ReadShip => "esi-location.read_ship_type.v1",
            EsiScope::ReadOnlineStatus => "esi-location.read_online.v1",
            EsiScope::WriteWaypoint => "esi-ui.write_waypoint.v1",
        }
    }

    /// The matching case name in the app's PHP EsiScope enum.
    pub fn php_case(self) -> String {
        format!("{self:?}")
    }
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Production,
    Local,
}

pub struct Answers {
    /// Host port the app binds in local mode (80 unless remapped).
    pub app_port: u16,
    /// Host port Reverb binds in local mode (8080 unless remapped).
    pub reverb_port: u16,
    pub app_domain: String,
    pub ws_domain: String,
    pub acme_email: String,
    pub contact_email: String,
    pub eve_client_id: String,
    pub eve_client_secret: String,
    pub allowed_affiliations: String,
    pub db_database: String,
    pub db_username: String,
    pub db_password: String,
    pub db_root_password: String,
}

pub fn local_app_url(app_port: u16) -> String {
    if app_port == 80 {
        "http://localhost".to_string()
    } else {
        format!("http://localhost:{app_port}")
    }
}

/// Every .env key the wizard manages; the integration tests verify each
/// still exists in the upstream example templates.
pub fn env_values(mode: Mode, a: &Answers) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let mut set = |k: &str, v: &str| values.insert(k.to_string(), v.to_string());
    set("APP_DOMAIN", &a.app_domain);
    set("WS_DOMAIN", &a.ws_domain);
    set("ACME_EMAIL", &a.acme_email);
    set("APP_KEY", &secrets::laravel_app_key());
    set("CONTACT_EMAIL", &a.contact_email);
    set("DB_DATABASE", &a.db_database);
    set("DB_USERNAME", &a.db_username);
    set("DB_PASSWORD", &a.db_password);
    set("REVERB_APP_ID", &secrets::hex(16));
    set("REVERB_APP_KEY", &secrets::hex(32));
    set("REVERB_APP_SECRET", &secrets::hex(32));
    set("EVE_CLIENT_ID", &a.eve_client_id);
    set("EVE_CLIENT_SECRET", &a.eve_client_secret);
    set("ALLOWED_AFFILIATION_IDS", &a.allowed_affiliations);
    match mode {
        Mode::Production => {
            set("APP_URL", &format!("https://{}", a.app_domain));
            set("VITE_REVERB_HOST", &a.ws_domain);
        }
        Mode::Local => {
            set("APP_URL", &local_app_url(a.app_port));
            set("VITE_REVERB_PORT", &a.reverb_port.to_string());
        }
    }
    values
}

pub struct PlannedFile {
    pub rel_path: &'static str,
    pub content: String,
}

pub enum Action {
    EnsureWebNetwork,
    Command { program: String, args: Vec<String> },
}

/// The two batches the user confirms separately.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StepGroup {
    Stack,
    Init,
}

/// A resumable unit of work; completed ids are persisted across runs.
pub struct Step {
    pub id: &'static str,
    pub group: StepGroup,
    pub actions: Vec<Action>,
}

fn custom_ports(mode: Mode, app_port: u16, reverb_port: u16) -> bool {
    mode == Mode::Local && (app_port != 80 || reverb_port != 8080)
}

/// Local test mode bypasses docker-compose.yml (whose includes pull in
/// the traefik/prod stack) by targeting the test file directly.
pub fn compose_files(mode: Mode, app_port: u16, reverb_port: u16) -> Vec<String> {
    match mode {
        Mode::Production => vec![],
        Mode::Local => {
            let mut files = vec!["docker-compose.test.yml".to_string()];
            if custom_ports(mode, app_port, reverb_port) {
                files.push(PORTS_OVERRIDE_FILE.to_string());
            }
            files
        }
    }
}

pub fn build_files(mode: Mode, a: &Answers, env_template: &str) -> Vec<PlannedFile> {
    let mut files = vec![
        PlannedFile {
            rel_path: ".env",
            content: envfile::patch(env_template, &env_values(mode, a)),
        },
        PlannedFile {
            rel_path: "dockerfiles/mysql/.env",
            content: format!(
                "MYSQL_ROOT_PASSWORD={}\nMYSQL_DATABASE={}\nMYSQL_USER={}\nMYSQL_PASSWORD={}\n",
                a.db_root_password, a.db_database, a.db_username, a.db_password
            ),
        },
    ];
    if custom_ports(mode, a.app_port, a.reverb_port) {
        files.push(PlannedFile {
            rel_path: PORTS_OVERRIDE_FILE,
            // `!override` replaces the hardcoded port list instead of
            // merging with it; needs docker compose v2.24+.
            content: format!(
                "# Generated by wsctl — remaps host ports to avoid conflicts with other\n\
                 # services on this machine. Safe to delete; wsctl setup regenerates it.\n\
                 services:\n\
                \x20 app:\n\
                \x20   ports: !override\n\
                \x20     - \"{}:80\"\n\
                \x20 reverb:\n\
                \x20   ports: !override\n\
                \x20     - \"{}:8080\"\n",
                a.app_port, a.reverb_port
            ),
        });
    }
    files
}

fn compose_action(files: &[String], tail: &[&str]) -> Action {
    let mut args = vec!["compose".to_string()];
    for file in files {
        args.push("-f".to_string());
        args.push(file.clone());
    }
    args.extend(tail.iter().map(|s| s.to_string()));
    Action::Command {
        program: "docker".to_string(),
        args,
    }
}

fn artisan_action(files: &[String], tail: &[&str]) -> Action {
    let mut args = vec!["exec", "app", "php", "artisan"];
    args.extend_from_slice(tail);
    compose_action(files, &args)
}

/// Takes only mode and ports so a resumed run can rebuild the step list
/// from persisted state without re-asking anything.
pub fn build_steps(mode: Mode, app_port: u16, reverb_port: u16) -> Vec<Step> {
    let files = compose_files(mode, app_port, reverb_port);
    let compose = |tail: &[&str]| compose_action(&files, tail);
    let artisan = |tail: &[&str]| artisan_action(&files, tail);

    let mut steps = Vec::new();
    if mode == Mode::Production {
        steps.push(Step {
            id: "network",
            group: StepGroup::Stack,
            actions: vec![Action::EnsureWebNetwork],
        });
    }
    steps.push(Step {
        id: "build",
        group: StepGroup::Stack,
        actions: vec![compose(&["build"])],
    });
    steps.push(Step {
        id: "up",
        group: StepGroup::Stack,
        actions: vec![compose(&["up", "-d"]), compose(&["ps"])],
    });
    steps.push(Step {
        id: "sde",
        group: StepGroup::Init,
        actions: vec![artisan(&["sde:download"])],
    });
    steps.push(Step {
        id: "migrate",
        group: StepGroup::Init,
        actions: vec![artisan(&["migrate", "--seed", "--force"])],
    });
    steps.push(Step {
        id: "optimize",
        group: StepGroup::Init,
        actions: vec![artisan(&["optimize:clear"]), artisan(&["optimize"])],
    });
    steps
}

/// The game-data update sequence from the upstream README. Custom port
/// mappings are irrelevant for `exec`, so default ports suffice.
pub fn update_actions(mode: Mode) -> Vec<Action> {
    let files = compose_files(mode, 80, 8080);
    vec![
        artisan_action(&files, &["sde:download"]),
        artisan_action(&files, &["migrate", "--force"]),
        artisan_action(&files, &["sde:seed"]),
    ]
}

/// Which stack a configured repo runs, read from its .env.
pub fn mode_from_env(env: &str) -> Option<Mode> {
    env.lines().find_map(|line| {
        let value = line.trim().strip_prefix("APP_ENV=")?;
        Some(match value.trim().trim_matches('"') {
            "local" | "testing" => Mode::Local,
            _ => Mode::Production,
        })
    })
}

/// Host ports the stack will bind.
pub fn stack_ports(mode: Mode, app_port: u16, reverb_port: u16) -> Vec<u16> {
    match mode {
        // Not remappable: Let's Encrypt's HTTP challenge requires 80/443.
        Mode::Production => vec![80, 443],
        Mode::Local => vec![app_port, reverb_port],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers(app_port: u16, reverb_port: u16) -> Answers {
        Answers {
            app_port,
            reverb_port,
            app_domain: "localhost".into(),
            ws_domain: format!("localhost:{reverb_port}"),
            acme_email: "a@b.c".into(),
            contact_email: "a@b.c | X".into(),
            eve_client_id: "id".into(),
            eve_client_secret: "secret".into(),
            allowed_affiliations: String::new(),
            db_database: "db".into(),
            db_username: "user".into(),
            db_password: "pass".into(),
            db_root_password: "root".into(),
        }
    }

    fn command_args(action: &Action) -> &[String] {
        match action {
            Action::Command { args, .. } => args,
            Action::EnsureWebNetwork => panic!("expected a command"),
        }
    }

    #[test]
    fn local_steps_use_test_compose_file() {
        let steps = build_steps(Mode::Local, 80, 8080);
        assert!(steps.iter().all(|s| s.id != "network"));
        let build = steps.iter().find(|s| s.id == "build").unwrap();
        assert_eq!(
            command_args(&build.actions[0]),
            &["compose", "-f", "docker-compose.test.yml", "build"]
        );
    }

    #[test]
    fn custom_ports_add_override_to_every_command_and_files() {
        let steps = build_steps(Mode::Local, 8000, 8081);
        let up = steps.iter().find(|s| s.id == "up").unwrap();
        assert_eq!(
            command_args(&up.actions[0]),
            &[
                "compose",
                "-f",
                "docker-compose.test.yml",
                "-f",
                PORTS_OVERRIDE_FILE,
                "up",
                "-d"
            ]
        );
        let files = build_files(Mode::Local, &answers(8000, 8081), "APP_URL=x\n");
        let override_file = files
            .iter()
            .find(|f| f.rel_path == PORTS_OVERRIDE_FILE)
            .expect("override planned");
        assert!(override_file.content.contains("\"8000:80\""));
        assert!(override_file.content.contains("\"8081:8080\""));
    }

    #[test]
    fn default_local_ports_plan_no_override() {
        let files = build_files(Mode::Local, &answers(80, 8080), "APP_URL=x\n");
        assert!(files.iter().all(|f| f.rel_path != PORTS_OVERRIDE_FILE));
        assert_eq!(files[0].rel_path, ".env");
        assert_eq!(files[1].rel_path, "dockerfiles/mysql/.env");
    }

    #[test]
    fn production_plans_network_and_bare_compose() {
        let steps = build_steps(Mode::Production, 80, 8080);
        assert_eq!(steps[0].id, "network");
        assert!(matches!(steps[0].actions[0], Action::EnsureWebNetwork));
        let build = steps.iter().find(|s| s.id == "build").unwrap();
        assert_eq!(command_args(&build.actions[0]), &["compose", "build"]);
    }

    #[test]
    fn update_actions_follow_upstream_readme_sequence() {
        let actions = update_actions(Mode::Local);
        let commands: Vec<_> = actions.iter().map(command_args).collect();
        assert_eq!(commands.len(), 3);
        let base = [
            "compose",
            "-f",
            "docker-compose.test.yml",
            "exec",
            "app",
            "php",
            "artisan",
        ];
        for (action, tail) in commands.iter().zip([
            &["sde:download"][..],
            &["migrate", "--force"],
            &["sde:seed"],
        ]) {
            let expected: Vec<&str> = base.iter().chain(tail).copied().collect();
            assert_eq!(action, &expected);
        }
        assert_eq!(
            command_args(&update_actions(Mode::Production)[0]),
            &["compose", "exec", "app", "php", "artisan", "sde:download"]
        );
    }

    #[test]
    fn mode_is_detected_from_app_env() {
        assert!(matches!(
            mode_from_env("APP_ENV=local\n"),
            Some(Mode::Local)
        ));
        assert!(matches!(
            mode_from_env("APP_NAME=x\nAPP_ENV=production\n"),
            Some(Mode::Production)
        ));
        assert!(mode_from_env("APP_NAME=x\n").is_none());
    }

    #[test]
    fn init_steps_run_artisan_inside_app_container() {
        let steps = build_steps(Mode::Local, 80, 8080);
        let sde = steps.iter().find(|s| s.id == "sde").unwrap();
        assert_eq!(sde.group, StepGroup::Init);
        assert_eq!(
            command_args(&sde.actions[0]),
            &[
                "compose",
                "-f",
                "docker-compose.test.yml",
                "exec",
                "app",
                "php",
                "artisan",
                "sde:download"
            ]
        );
    }
}
