use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;
use std::time::Instant;

use crate::config::{Action, ActionType, Config, OnFailure};
use crate::logger::Logger;

// ── Background process tracking ───────────────────────────────────────────────

struct Background {
    name: String,
    child: Child,
    start: Instant,
}

// ── Executor ──────────────────────────────────────────────────────────────────

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

    // ── Entry point ───────────────────────────────────────────────────────

    pub fn run(&mut self, config: &Config) -> Result<()> {
        let name = config.name.as_deref().unwrap_or("Deployment");
        self.logger.header(name);

        // Apply global environment variables.
        if let Some(global_env) = &config.env {
            self.logger.section("Global environment");
            for (k, v) in global_env {
                let v = self.substitute(v);
                self.env.insert(k.clone(), v.clone());
                // Propagate into the current process so child processes can
                // inherit via the OS environment.  This must only be called
                // from a single-threaded context; the executor is deliberately
                // single-threaded (background work is delegated to child
                // processes, not Rust threads) so this is safe.
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
                Ok(ActionOutcome::Background) => { /* counted when joined */ }
                Ok(ActionOutcome::Skipped) => skipped += 1,
                Err(e) => {
                    self.logger.action_error(&label, &e.to_string());
                    failed += 1;
                    let policy = action.on_failure.unwrap_or_default();
                    if policy == OnFailure::Stop {
                        // Still wait for any background tasks before exiting.
                        let (bg_ok, bg_fail) = self.join_backgrounds(backgrounds);
                        success += bg_ok;
                        failed += bg_fail;
                        self.logger.footer(total, success, failed, skipped);
                        return Err(anyhow!("Stopping after failure in action '{label}'"));
                    }
                }
            }
        }

        // Join all still-running background processes.
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

    // ── Action dispatch ───────────────────────────────────────────────────

    fn execute_action(
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

    // ── run ───────────────────────────────────────────────────────────────

    fn run_action(
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

    // ── shell ─────────────────────────────────────────────────────────────

    fn shell_action(
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

    // ── set_env ───────────────────────────────────────────────────────────

    fn set_env_action(&mut self, action: &Action, label: &str) -> Result<ActionOutcome> {
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

    // ── unset_env ─────────────────────────────────────────────────────────

    fn unset_env_action(&mut self, action: &Action, label: &str) -> Result<ActionOutcome> {
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

    // ── wait ──────────────────────────────────────────────────────────────

    fn wait_action(&mut self, action: &Action, label: &str, is_bg: bool) -> Result<ActionOutcome> {
        if is_bg {
            return Err(anyhow!("'wait' action does not support 'background: true'"));
        }

        let mode_count = [
            action.duration_ms.is_some(),
            action.until_file_exists.is_some(),
            action.until_http_ok.is_some(),
        ]
        .into_iter()
        .filter(|enabled| *enabled)
        .count();

        if mode_count != 1 {
            return Err(anyhow!(
                "'wait' action requires exactly one of 'duration_ms', 'until_file_exists', or 'until_http_ok'"
            ));
        }

        match (
            &action.duration_ms,
            &action.until_file_exists,
            &action.until_http_ok,
        ) {
            (Some(duration_ms), None, None) => {
                self.logger
                    .action_start("wait", label, &format!("{duration_ms}ms"));

                if self.dry_run {
                    self.logger.dry_run(&format!("wait: {duration_ms}ms"));
                    return Ok(ActionOutcome::Skipped);
                }

                let start = Instant::now();
                thread::sleep(Duration::from_millis(*duration_ms));
                self.logger
                    .action_success(label, start.elapsed().as_millis() as u64);
                Ok(ActionOutcome::Success)
            }
            (None, Some(path), None) => {
                let target = self.substitute(path);
                let timeout_ms = action.timeout_ms.unwrap_or(30_000);
                let interval_ms = action.interval_ms.unwrap_or(200);

                if interval_ms == 0 {
                    return Err(anyhow!("'wait' action requires 'interval_ms' to be greater than 0"));
                }

                self.logger.action_start(
                    "wait",
                    label,
                    &format!(
                        "until file exists: {target} (timeout={timeout_ms}ms, interval={interval_ms}ms)"
                    ),
                );

                if self.dry_run {
                    self.logger.dry_run(&format!(
                        "wait until file exists: {target} (timeout={timeout_ms}ms, interval={interval_ms}ms)"
                    ));
                    return Ok(ActionOutcome::Skipped);
                }

                let start = Instant::now();
                loop {
                    if Path::new(&target).exists() {
                        self.logger
                            .action_success(label, start.elapsed().as_millis() as u64);
                        return Ok(ActionOutcome::Success);
                    }

                    let elapsed_ms = start.elapsed().as_millis() as u64;
                    if elapsed_ms >= timeout_ms {
                        return Err(anyhow!(
                            "timed out after {timeout_ms}ms waiting for file '{target}'"
                        ));
                    }

                    let remaining_ms = timeout_ms.saturating_sub(elapsed_ms);
                    thread::sleep(Duration::from_millis(interval_ms.min(remaining_ms)));
                }
            }
            (None, None, Some(url)) => {
                let target = self.substitute(url);
                let timeout_ms = action.timeout_ms.unwrap_or(30_000);
                let interval_ms = action.interval_ms.unwrap_or(200);

                if interval_ms == 0 {
                    return Err(anyhow!("'wait' action requires 'interval_ms' to be greater than 0"));
                }

                self.logger.action_start(
                    "wait",
                    label,
                    &format!(
                        "until HTTP OK: {target} (timeout={timeout_ms}ms, interval={interval_ms}ms)"
                    ),
                );

                if self.dry_run {
                    self.logger.dry_run(&format!(
                        "wait until HTTP OK: {target} (timeout={timeout_ms}ms, interval={interval_ms}ms)"
                    ));
                    return Ok(ActionOutcome::Skipped);
                }

                let start = Instant::now();
                loop {
                    let elapsed_ms = start.elapsed().as_millis() as u64;
                    if elapsed_ms >= timeout_ms {
                        return Err(anyhow!(
                            "timed out after {timeout_ms}ms waiting for HTTP OK from '{target}'"
                        ));
                    }

                    let remaining_ms = timeout_ms.saturating_sub(elapsed_ms);
                    let request_timeout_ms = remaining_ms.min(interval_ms.max(1_000));
                    let client = Client::builder()
                        .timeout(Duration::from_millis(request_timeout_ms.max(1)))
                        .build()
                        .context("failed to build HTTP client for wait action")?;

                    if let Ok(response) = client.get(&target).send() {
                        if response.status().is_success() {
                            self.logger
                                .action_success(label, start.elapsed().as_millis() as u64);
                            return Ok(ActionOutcome::Success);
                        }
                    }

                    let elapsed_ms = start.elapsed().as_millis() as u64;
                    if elapsed_ms >= timeout_ms {
                        return Err(anyhow!(
                            "timed out after {timeout_ms}ms waiting for HTTP OK from '{target}'"
                        ));
                    }

                    let remaining_ms = timeout_ms.saturating_sub(elapsed_ms);
                    thread::sleep(Duration::from_millis(interval_ms.min(remaining_ms)));
                }
            }
            _ => unreachable!("mode_count validation ensures exactly one wait mode"),
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    /// Spawn a process either in the background or wait for it to finish.
    fn spawn_or_wait(
        &self,
        mut cmd: Command,
        label: &str,
        is_bg: bool,
        context_name: &str,
        backgrounds: &mut Vec<Background>,
    ) -> Result<ActionOutcome> {
        let start = Instant::now();

        if is_bg {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
            let child = cmd
                .spawn()
                .with_context(|| format!("Failed to spawn '{context_name}'"))?;
            backgrounds.push(Background {
                name: label.to_string(),
                child,
                start,
            });
            self.logger.action_background(label);
            return Ok(ActionOutcome::Background);
        }

        let output = cmd
            .output()
            .with_context(|| format!("Failed to execute '{context_name}'"))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        if self.logger.is_verbose() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                self.logger.output_line(line);
            }
        }

        if output.status.success() {
            self.logger.action_success(label, duration_ms);
            Ok(ActionOutcome::Success)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow!(
                "exited with {}: {}",
                output.status,
                stderr.trim()
            ))
        }
    }

    /// Apply accumulated env + per-action env overrides to `cmd`.
    fn apply_env(&self, cmd: &mut Command, action: &Action) {
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

    /// Replace `${VAR}` placeholders with values from the accumulated
    /// environment map, then from the OS environment.
    ///
    /// Only the `${VAR}` form is supported; bare `$VAR` is intentionally *not*
    /// expanded because it is ambiguous when variable names share prefixes
    /// (e.g. `$FOO` vs `$FOOBAR`).
    fn substitute(&self, s: &str) -> String {
        let mut out = s.to_string();
        // Internal env takes priority.
        for (k, v) in &self.env {
            out = out.replace(&format!("${{{k}}}"), v);
        }
        // Fall back to OS env for anything still unreplaced.
        for (k, v) in std::env::vars() {
            let placeholder = format!("${{{k}}}");
            if out.contains(&placeholder) {
                out = out.replace(&placeholder, &v);
            }
        }
        out
    }

    /// Wait for all background processes and return (success_count, failure_count).
    fn join_backgrounds(&self, backgrounds: Vec<Background>) -> (usize, usize) {
        if backgrounds.is_empty() {
            return (0, 0);
        }
        self.logger.section("Waiting for background processes");
        let mut ok = 0usize;
        let mut fail = 0usize;
        for mut bg in backgrounds {
            self.logger.info(&format!("Waiting for '{}' …", bg.name));
            match bg.child.wait() {
                Ok(status) => {
                    let ms = bg.start.elapsed().as_millis() as u64;
                    if status.success() {
                        self.logger.action_success(&bg.name, ms);
                        ok += 1;
                    } else {
                        self.logger
                            .action_error(&bg.name, &format!("exited with {status}"));
                        fail += 1;
                    }
                }
                Err(e) => {
                    self.logger.action_error(&bg.name, &e.to_string());
                    fail += 1;
                }
            }
        }
        (ok, fail)
    }
}

// ── Outcome ───────────────────────────────────────────────────────────────────

enum ActionOutcome {
    Success,
    Background,
    Skipped,
}
