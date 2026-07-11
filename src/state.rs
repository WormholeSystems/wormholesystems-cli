//! Progress of an interrupted setup, persisted as `.wsctl-state.json` in
//! the repo so the next run can resume. Deliberately contains no secrets —
//! those live only in the .env files, written before the first step.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::plan::Mode;

const STATE_FILE: &str = ".wsctl-state.json";

#[derive(Serialize, Deserialize)]
pub struct ResumeState {
    pub mode: Mode,
    pub app_domain: String,
    pub ws_domain: String,
    pub app_port: u16,
    pub reverb_port: u16,
    /// Ids of completed steps (see plan::build_steps).
    pub completed: Vec<String>,
    #[serde(skip)]
    path: PathBuf,
}

impl ResumeState {
    pub fn new(repo: &Path, mode: Mode, a: &crate::plan::Answers) -> Self {
        Self {
            mode,
            app_domain: a.app_domain.clone(),
            ws_domain: a.ws_domain.clone(),
            app_port: a.app_port,
            reverb_port: a.reverb_port,
            completed: Vec::new(),
            path: repo.join(STATE_FILE),
        }
    }

    pub fn load(repo: &Path) -> Result<Option<Self>> {
        let path = repo.join(STATE_FILE);
        let Ok(raw) = fs::read_to_string(&path) else {
            return Ok(None);
        };
        let mut state: Self = serde_json::from_str(&raw)
            .with_context(|| format!("{} is corrupt — delete it to start fresh", path.display()))?;
        state.path = path;
        Ok(Some(state))
    }

    pub fn is_done(&self, step_id: &str) -> bool {
        self.completed.iter().any(|s| s == step_id)
    }

    /// Persists immediately, so progress survives a crash.
    pub fn mark_done(&mut self, step_id: &str) -> Result<()> {
        if !self.is_done(step_id) {
            self.completed.push(step_id.to_string());
        }
        self.save()
    }

    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&self.path, json).with_context(|| format!("cannot write {}", self.path.display()))
    }

    pub fn delete(self) -> Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)
                .with_context(|| format!("cannot delete {}", self.path.display()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Answers, Mode};

    fn answers() -> Answers {
        Answers {
            app_port: 8000,
            reverb_port: 8081,
            app_domain: "localhost".into(),
            ws_domain: "localhost:8081".into(),
            acme_email: String::new(),
            contact_email: String::new(),
            eve_client_id: String::new(),
            eve_client_secret: "secret".into(),
            allowed_affiliations: String::new(),
            db_database: String::new(),
            db_username: String::new(),
            db_password: "dbpass".into(),
            db_root_password: String::new(),
        }
    }

    #[test]
    fn roundtrip_persists_progress_but_no_secrets() {
        let dir = std::env::temp_dir().join(format!("wsctl-state-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let mut state = ResumeState::new(&dir, Mode::Local, &answers());
        state.mark_done("build").unwrap();

        let raw = fs::read_to_string(dir.join(STATE_FILE)).unwrap();
        assert!(!raw.contains("secret") && !raw.contains("dbpass"));

        let loaded = ResumeState::load(&dir).unwrap().expect("state exists");
        assert!(loaded.is_done("build"));
        assert!(!loaded.is_done("up"));
        assert_eq!(loaded.app_port, 8000);

        loaded.delete().unwrap();
        assert!(ResumeState::load(&dir).unwrap().is_none());
        fs::remove_dir_all(&dir).ok();
    }
}
