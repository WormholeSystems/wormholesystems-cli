//! Carries out planned steps through the `Runner` trait, so tests can
//! assert exactly which commands would run without touching docker.

use std::path::Path;

use anyhow::Result;
use console::style;

use crate::docker;
use crate::plan::{Action, Step};
use crate::state::ResumeState;

pub trait Runner {
    fn run(&mut self, dir: &Path, program: &str, args: &[String]) -> Result<()>;
    fn ensure_web_network(&mut self, dir: &Path) -> Result<()>;
}

pub struct RealRunner;

impl Runner for RealRunner {
    fn run(&mut self, dir: &Path, program: &str, args: &[String]) -> Result<()> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        docker::run(dir, program, &args)
    }

    fn ensure_web_network(&mut self, dir: &Path) -> Result<()> {
        docker::ensure_web_network(dir)
    }
}

/// Skips steps the state records as done; persists after each completion.
pub fn run_steps(
    runner: &mut dyn Runner,
    repo: &Path,
    steps: &[&Step],
    state: &mut ResumeState,
) -> Result<()> {
    for step in steps {
        if state.is_done(step.id) {
            println!(
                "{} step `{}` already completed — skipping.",
                style("✓").green(),
                step.id
            );
            continue;
        }
        for action in &step.actions {
            match action {
                Action::EnsureWebNetwork => runner.ensure_web_network(repo)?,
                Action::Command { program, args } => runner.run(repo, program, args)?,
            }
        }
        state.mark_done(step.id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Mode, StepGroup, build_steps};

    #[derive(Default)]
    struct MockRunner {
        commands: Vec<String>,
        networks_ensured: usize,
    }

    impl Runner for MockRunner {
        fn run(&mut self, _dir: &Path, program: &str, args: &[String]) -> Result<()> {
            self.commands.push(format!("{program} {}", args.join(" ")));
            Ok(())
        }

        fn ensure_web_network(&mut self, _dir: &Path) -> Result<()> {
            self.networks_ensured += 1;
            Ok(())
        }
    }

    fn test_state(dir: &Path) -> ResumeState {
        let answers = crate::plan::Answers {
            app_port: 80,
            reverb_port: 8080,
            app_domain: "localhost".into(),
            ws_domain: "localhost:8080".into(),
            acme_email: String::new(),
            contact_email: String::new(),
            eve_client_id: String::new(),
            eve_client_secret: String::new(),
            allowed_affiliations: String::new(),
            db_database: String::new(),
            db_username: String::new(),
            db_password: String::new(),
            db_root_password: String::new(),
        };
        ResumeState::new(dir, Mode::Local, &answers)
    }

    fn temp_repo(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wsctl-exec-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn completed_steps_are_skipped_on_resume() {
        let dir = temp_repo("skip");
        let steps = build_steps(Mode::Local, 80, 8080);
        let stack: Vec<&_> = steps
            .iter()
            .filter(|s| s.group == StepGroup::Stack)
            .collect();

        let mut state = test_state(&dir);
        state.mark_done("build").unwrap();

        let mut runner = MockRunner::default();
        run_steps(&mut runner, &dir, &stack, &mut state).unwrap();

        // Only `up` ran (its two commands); `build` was skipped.
        assert_eq!(
            runner.commands,
            vec![
                "docker compose -f docker-compose.test.yml up -d",
                "docker compose -f docker-compose.test.yml ps",
            ]
        );
        assert!(state.is_done("up"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn production_stack_ensures_network_once() {
        let dir = temp_repo("network");
        let steps = build_steps(Mode::Production, 80, 8080);
        let stack: Vec<&_> = steps
            .iter()
            .filter(|s| s.group == StepGroup::Stack)
            .collect();

        let mut state = test_state(&dir);
        let mut runner = MockRunner::default();
        run_steps(&mut runner, &dir, &stack, &mut state).unwrap();

        assert_eq!(runner.networks_ensured, 1);
        assert_eq!(
            runner.commands,
            vec![
                "docker compose build",
                "docker compose up -d",
                "docker compose ps",
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
