use std::process::Command;

use crate::config::Action;

use super::Executor;

impl Executor {
    /// Apply accumulated env + per-action env overrides to `cmd`.
    pub(super) fn apply_env(&self, cmd: &mut Command, action: &Action) {
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        if let Some(overrides) = &action.env {
            for (k, v) in overrides {
                let v = self.substitute(v);
                cmd.env(k, &v);
            }
        }
    }

    /// Replace `${VAR}` placeholders with values from accumulated env,
    /// then from OS env.
    pub(super) fn substitute(&self, s: &str) -> String {
        let mut out = s.to_string();
        for (k, v) in &self.env {
            out = out.replace(&format!("${{{k}}}"), v);
        }
        for (k, v) in std::env::vars() {
            let placeholder = format!("${{{k}}}");
            if out.contains(&placeholder) {
                out = out.replace(&placeholder, &v);
            }
        }
        out
    }
}
