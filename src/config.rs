use serde::Deserialize;
use std::collections::HashMap;

/// Top-level configuration loaded from the JSON file.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// Optional human-readable name for this deployment.
    pub name: Option<String>,
    /// Global environment variables applied to every action.
    pub env: Option<HashMap<String, String>>,
    /// Ordered list of actions to execute.
    pub actions: Vec<Action>,
}

/// A single action in the deployment plan.
#[derive(Debug, Deserialize)]
pub struct Action {
    /// Human-readable label for this action.
    pub name: Option<String>,
    /// Discriminator field that selects the action kind.
    #[serde(rename = "type")]
    pub action_type: ActionType,
    /// For `run` and `shell`: the program or shell expression to execute.
    pub command: Option<String>,
    /// For `run`: positional arguments appended after the command.
    pub args: Option<Vec<String>>,
    /// Per-action environment overrides (layered on top of global env).
    pub env: Option<HashMap<String, String>>,
    /// Working directory for the spawned process.
    pub working_dir: Option<String>,
    /// When `true` the process is spawned in the background and the next
    /// action starts immediately.  All background processes are joined at the
    /// end of the run.
    pub background: Option<bool>,
    /// For `set_env` / `unset_env`: the variable name.
    pub key: Option<String>,
    /// For `set_env`: the variable value.
    pub value: Option<String>,
    /// What to do when this action fails: `"stop"` (default) or `"continue"`.
    pub on_failure: Option<OnFailure>,
}

/// Determines how to react when an action exits with a non-zero status.
#[derive(Debug, Default, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum OnFailure {
    #[default]
    Stop,
    Continue,
}

/// The kind of action to perform.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    /// Run an executable with optional arguments.
    Run,
    /// Execute an arbitrary shell expression.
    Shell,
    /// Set (or override) an environment variable for subsequent actions.
    SetEnv,
    /// Remove an environment variable for subsequent actions.
    UnsetEnv,
}
