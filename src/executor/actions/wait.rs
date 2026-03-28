use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Action;

use super::super::{ActionOutcome, Executor};

impl Executor {
    pub(crate) fn wait_action(
        &mut self,
        action: &Action,
        label: &str,
        is_bg: bool,
    ) -> Result<ActionOutcome> {
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
}
