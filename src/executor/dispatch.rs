use anyhow::Result;

use crate::config::{Action, ActionType};

use super::{ActionOutcome, Background, Executor};

impl Executor {
    pub(super) fn execute_action(
        &mut self,
        action: &Action,
        label: &str,
        backgrounds: &mut Vec<Background>,
    ) -> Result<ActionOutcome> {
        let is_bg = action.background.unwrap_or(false);

        match &action.action_type {
            ActionType::Run => self.run_action(action, label, is_bg, backgrounds),
            ActionType::Shell => self.shell_action(action, label, is_bg, backgrounds),
            ActionType::Wait => self.wait_action(action, label, is_bg),
            ActionType::SetEnv => self.set_env_action(action, label),
            ActionType::UnsetEnv => self.unset_env_action(action, label),
        }
    }
}
