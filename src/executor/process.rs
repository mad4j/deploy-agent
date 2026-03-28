use anyhow::{anyhow, Context, Result};
use std::process::{Command, Stdio};
use std::time::Instant;

use super::{ActionOutcome, Background, Executor};

impl Executor {
    /// Spawn a process either in the background or wait for it to finish.
    pub(super) fn spawn_or_wait(
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
            Err(anyhow!("exited with {}: {}", output.status, stderr.trim()))
        }
    }

    /// Wait for all background processes and return (success_count, failure_count).
    pub(super) fn join_backgrounds(&self, backgrounds: Vec<Background>) -> (usize, usize) {
        if backgrounds.is_empty() {
            return (0, 0);
        }
        self.logger.section("Waiting for background processes");
        let mut ok = 0usize;
        let mut fail = 0usize;
        for mut bg in backgrounds {
            self.logger.info(&format!("Waiting for '{}' ...", bg.name));
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
