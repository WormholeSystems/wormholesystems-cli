//! Drift tests against the real upstream repo (vendored as a submodule):
//! fail when upstream changes something the wizard relies on.

use std::fs;
use std::path::PathBuf;

use wormholesystems_cli::envfile;
use wormholesystems_cli::plan::{Answers, Mode, env_values};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("upstream/wormholesystems-containers")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing fixture {} — run `git submodule update --init --recursive` first",
            path.display()
        )
    })
}

fn answers() -> Answers {
    Answers {
        app_port: 80,
        reverb_port: 8080,
        app_domain: "example.com".into(),
        ws_domain: "ws.example.com".into(),
        acme_email: "acme@example.com".into(),
        contact_email: "me@example.com | My Character".into(),
        eve_client_id: "clientid".into(),
        eve_client_secret: "clientsecret".into(),
        allowed_affiliations: String::new(),
        db_database: "wormholesystems".into(),
        db_username: "wormholesystems".into(),
        db_password: "dbpass".into(),
        db_root_password: "rootpass".into(),
    }
}

/// A key in the appended "# Added by wsctl" block means upstream renamed
/// or dropped a key the wizard manages.
fn assert_all_keys_patched(template_name: &str, mode: Mode) {
    let template = fixture(template_name);
    let values = env_values(mode, &answers());
    let patched = envfile::patch(&template, &values);

    assert!(
        !patched.contains("# Added by wsctl"),
        "{template_name} no longer contains some keys the wizard manages:\n{}",
        patched.split("# Added by wsctl").nth(1).unwrap_or("")
    );
    assert!(
        !patched.contains("<fill in"),
        "{template_name} still has unfilled placeholders after patching — \
         upstream added a new required key the wizard does not cover:\n{}",
        patched
            .lines()
            .filter(|l| l.contains("<fill in"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn production_template_covers_all_managed_keys() {
    assert_all_keys_patched(".env.production.example", Mode::Production);
}

#[test]
fn local_template_covers_all_managed_keys() {
    assert_all_keys_patched(".env.local.example", Mode::Local);
}

#[test]
fn local_custom_ports_flow_into_env_values() {
    let mut a = answers();
    a.app_port = 8000;
    a.reverb_port = 8081;
    a.ws_domain = "localhost:8081".into();
    let values = env_values(Mode::Local, &a);
    assert_eq!(values["APP_URL"], "http://localhost:8000");
    assert_eq!(values["VITE_REVERB_PORT"], "8081");
    assert_eq!(values["WS_DOMAIN"], "localhost:8081");

    a.app_port = 80;
    let values = env_values(Mode::Local, &a);
    assert_eq!(values["APP_URL"], "http://localhost");
}

#[test]
fn mysql_env_example_keys_are_covered() {
    let example = fixture("dockerfiles/mysql/.env.example");
    let written = [
        "MYSQL_ROOT_PASSWORD",
        "MYSQL_DATABASE",
        "MYSQL_USER",
        "MYSQL_PASSWORD",
    ];
    for line in example.lines() {
        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        assert!(
            written.contains(&key.trim()),
            "upstream mysql .env.example has a key the wizard does not write: {key}"
        );
    }
}

#[test]
fn wizard_scope_guidance_matches_app_default_scopes() {
    use wormholesystems_cli::plan::EsiScope;

    let seeder = fixture("wormhole-systems/database/seeders/EsiScopeSeeder.php");
    let block = seeder
        .split("defaultScopes = [")
        .nth(1)
        .and_then(|rest| rest.split("];").next())
        .expect(
            "EsiScopeSeeder no longer defines $defaultScopes — update the wizard's scope guidance",
        );

    assert_eq!(
        block.matches("EsiScope::").count(),
        EsiScope::DEFAULTS.len(),
        "the app's default scope list changed — update plan::EsiScope:\n{block}"
    );
    for scope in EsiScope::DEFAULTS {
        assert!(
            block.contains(&scope.php_case()),
            "app no longer requests EsiScope::{} by default — update plan::EsiScope",
            scope.php_case()
        );
    }
}

#[test]
fn frontend_references_no_scopes_the_wizard_omits() {
    use wormholesystems_cli::plan::EsiScope;

    let wizard_scopes: Vec<&str> = EsiScope::DEFAULTS.iter().map(|s| s.as_str()).collect();
    let js_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("upstream/wormholesystems-containers/wormhole-systems/resources/js");
    let mut stack = vec![js_dir];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(source) = fs::read_to_string(&path) {
                for scope in source
                    .split(|c: char| c.is_whitespace() || "\"'`,".contains(c))
                    .filter(|w| w.starts_with("esi-") && w.ends_with(".v1"))
                {
                    assert!(
                        wizard_scopes.contains(&scope),
                        "frontend references scope {scope} ({}) that the wizard guidance omits",
                        path.display()
                    );
                }
            }
        }
    }
}

/// Local mode relies on this guard; without it users need a manual edit.
#[test]
fn force_https_is_environment_guarded_upstream() {
    let provider = fixture("wormhole-systems/app/Providers/AppServiceProvider.php");
    if provider.contains("URL::forceHttps") {
        assert!(
            provider.contains("environment(['local'"),
            "AppServiceProvider calls URL::forceHttps() without the local/testing \
             environment guard — the wizard's local mode note needs updating"
        );
    }
}

/// The wizard writes .env and dockerfiles/mysql/.env; compose must still
/// read the container environment from exactly those paths.
#[test]
fn compose_env_file_paths_match_what_the_wizard_writes() {
    for compose in ["docker-compose.prod.yml", "docker-compose.test.yml"] {
        let content = fixture(compose);
        let referenced: Vec<&str> = content
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("- ") && l.contains(".env"))
            .map(|l| l.trim_start_matches("- ").trim())
            .collect();
        assert!(
            referenced.contains(&".env"),
            "{compose} no longer reads .env from the repo root"
        );
        assert!(
            referenced.contains(&"dockerfiles/mysql/.env"),
            "{compose} no longer reads dockerfiles/mysql/.env"
        );
        for path in referenced {
            assert!(
                [".env", "dockerfiles/mysql/.env"].contains(&path),
                "{compose} reads an env file the wizard does not write: {path}"
            );
        }
    }
}

#[test]
fn compose_files_still_define_expected_services() {
    let prod = fixture("docker-compose.prod.yml");
    for service in ["app:", "mysql:", "redis:", "reverb:"] {
        assert!(
            prod.contains(service),
            "docker-compose.prod.yml no longer defines service `{service}` the wizard relies on"
        );
    }
}
