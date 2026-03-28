use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::process::Child;
use std::time::Instant;

use crate::config::{Config, OnFailure};
use crate::logger::Logger;

mod actions;
mod dispatch;
mod env_utils;
mod process;

pub(super) struct Background {
    name: String,
    child: Child,
    start: Instant,
}

pub struct Executor {
    dry_run: bool,
    logger: Logger,
    /// Accumulated environment variables set by `set_env` actions.
    env: HashMap<String, String>,
}

impl Executor {
    pub fn new(dry_run: bool, verbose: bool) -> Self {
        Self {
            dry_run,
            logger: Logger::new(verbose),
            env: HashMap::new(),
        }
    }

    pub fn run(&mut self, config: &Config) -> Result<()> {
        let name = config.name.as_deref().unwrap_or("Deployment");
        self.logger.header(name);

        // Apply global environment variables.
        if let Some(global_env) = &config.env {
            self.logger.section("Global environment");
            for (k, v) in global_env {
                let v = self.substitute(v);
                self.env.insert(k.clone(), v.clone());
                // The executor is single-threaded, so updating process env is safe.
                std::env::set_var(k, &v);
                self.logger.env_set(k, &v);
            }
        }

        self.logger.section("Executing actions");

        let total = config.actions.len();
        let mut success = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;
        let mut backgrounds: Vec<Background> = Vec::new();

        for (i, action) in config.actions.iter().enumerate() {
            let label = action
                .name
                .clone()
                .unwrap_or_else(|| format!("action-{}", i + 1));

            self.logger
                .verbose(&format!("  [{}/{}] {label}", i + 1, total));

            match self.execute_action(action, &label, &mut backgrounds) {
                Ok(ActionOutcome::Success) => success += 1,
                Ok(ActionOutcome::Background) => {}
                Ok(ActionOutcome::Skipped) => skipped += 1,
                Err(e) => {
                    self.logger.action_error(&label, &e.to_string());
                    failed += 1;
                    let policy = action.on_failure.unwrap_or_default();
                    if policy == OnFailure::Stop {
                        let (bg_ok, bg_fail) = self.join_backgrounds(backgrounds);
                        success += bg_ok;
                        failed += bg_fail;
                        self.logger.footer(total, success, failed, skipped);
                        return Err(anyhow!("Stopping after failure in action '{label}'"));
                    }
                }
            }
        }

        let (bg_ok, bg_fail) = self.join_backgrounds(backgrounds);
        success += bg_ok;
        failed += bg_fail;

        self.logger.footer(total, success, failed, skipped);

        if failed > 0 {
            Err(anyhow!("{failed} action(s) failed"))
        } else {
            Ok(())
        }
    }

}

pub(super) enum ActionOutcome {
    Success,
    Background,
    Skipped,
}
