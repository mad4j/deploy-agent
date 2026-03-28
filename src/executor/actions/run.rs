use anyhow::{Context, Result};
use std::process::Command;

use crate::config::Action;

use super::super::{ActionOutcome, Background, Executor};

impl Executor {
    pub(crate) fn run_action(
        &mut self,
        action: &Action,
        label: &str,
        is_bg: bool,
        backgrounds: &mut Vec<Background>,
    ) -> Result<ActionOutcome> {
        let raw_cmd = action
            .command
            .as_ref()
            .context("'run' action requires a 'command' field")?;
        let program = self.substitute(raw_cmd);
        let args: Vec<String> = action
            .args
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|a| self.substitute(a))
            .collect();

        let display = format!("{} {}", program, args.join(" ")).trim().to_string();
        self.logger.action_start("run", label, &display);

        if self.dry_run {
            self.logger.dry_run(&format!("run: {display}"));
            return Ok(ActionOutcome::Skipped);
        }

        let mut cmd = Command::new(&program);
        cmd.args(&args);
        self.apply_env(&mut cmd, action);
        if let Some(wd) = &action.working_dir {
            cmd.current_dir(wd);
        }

        self.spawn_or_wait(cmd, label, is_bg, &program, backgrounds)
    }
}
