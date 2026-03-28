use anyhow::{Context, Result};

use crate::config::Action;

use super::super::{ActionOutcome, Executor};

impl Executor {
    pub(crate) fn set_env_action(&mut self, action: &Action, label: &str) -> Result<ActionOutcome> {
        let key = action
            .key
            .as_ref()
            .context("'set_env' action requires a 'key' field")?;
        let raw_value = action
            .value
            .as_ref()
            .context("'set_env' action requires a 'value' field")?;
        let value = self.substitute(raw_value);

        self.logger
            .action_start("env", label, &format!("set {key}={value}"));
        self.env.insert(key.clone(), value.clone());
        std::env::set_var(key, &value);
        self.logger.env_set(key, &value);
        self.logger.action_success(label, 0);
        Ok(ActionOutcome::Success)
    }

    pub(crate) fn unset_env_action(&mut self, action: &Action, label: &str) -> Result<ActionOutcome> {
        let key = action
            .key
            .as_ref()
            .context("'unset_env' action requires a 'key' field")?;

        self.logger
            .action_start("env", label, &format!("unset {key}"));
        self.env.remove(key);
        std::env::remove_var(key);
        self.logger.env_unset(key);
        self.logger.action_success(label, 0);
        Ok(ActionOutcome::Success)
    }
}
