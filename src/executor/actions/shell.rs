use anyhow::{Context, Result};
use std::process::Command;

use crate::config::Action;

use super::super::{ActionOutcome, Background, Executor};

impl Executor {
    pub(crate) fn shell_action(
        &mut self,
        action: &Action,
        label: &str,
        is_bg: bool,
        backgrounds: &mut Vec<Background>,
    ) -> Result<ActionOutcome> {
        let raw_cmd = action
            .command
            .as_ref()
            .context("'shell' action requires a 'command' field")?;
        let expr = self.substitute(raw_cmd);

        self.logger.action_start("shell", label, &expr);

        if self.dry_run {
            self.logger.dry_run(&format!("shell: {expr}"));
            return Ok(ActionOutcome::Skipped);
        }

        #[cfg(unix)]
        let (sh, flag) = ("sh", "-c");
        #[cfg(windows)]
        let (sh, flag) = ("cmd", "/C");

        let mut cmd = Command::new(sh);
        cmd.arg(flag).arg(&expr);
        self.apply_env(&mut cmd, action);
        if let Some(wd) = &action.working_dir {
            cmd.current_dir(wd);
        }

        self.spawn_or_wait(cmd, label, is_bg, &expr, backgrounds)
    }
}
