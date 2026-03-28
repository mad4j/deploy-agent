use anyhow::{anyhow, Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use crate::config::Action;

use super::super::{ActionOutcome, Executor};

impl Executor {
    pub(crate) fn mkdir_action(
        &mut self,
        action: &Action,
        label: &str,
        is_bg: bool,
    ) -> Result<ActionOutcome> {
        self.ensure_not_background("mkdir", is_bg)?;

        let path = action
            .path
            .as_ref()
            .context("'mkdir' action requires a 'path' field")?;
        let target = self.substitute(path);
        let recursive = action.recursive.unwrap_or(true);

        self.logger.action_start(
            "fs",
            label,
            &format!("mkdir {target} (recursive={recursive})"),
        );

        if self.dry_run {
            self.logger
                .dry_run(&format!("mkdir {target} (recursive={recursive})"));
            return Ok(ActionOutcome::Skipped);
        }

        let start = Instant::now();
        if recursive {
            fs::create_dir_all(&target)
                .with_context(|| format!("failed to create directory '{target}'"))?;
        } else {
            fs::create_dir(&target)
                .with_context(|| format!("failed to create directory '{target}'"))?;
        }

        self.logger
            .action_success(label, start.elapsed().as_millis() as u64);
        Ok(ActionOutcome::Success)
    }

    pub(crate) fn write_file_action(
        &mut self,
        action: &Action,
        label: &str,
        is_bg: bool,
    ) -> Result<ActionOutcome> {
        self.ensure_not_background("write_file", is_bg)?;

        let path = action
            .path
            .as_ref()
            .context("'write_file' action requires a 'path' field")?;
        let content = action
            .content
            .as_ref()
            .context("'write_file' action requires a 'content' field")?;

        let target = self.substitute(path);
        let value = self.substitute(content);
        let append = action.append.unwrap_or(false);

        self.logger.action_start(
            "fs",
            label,
            &format!("write_file {target} (append={append}, bytes={})", value.len()),
        );

        if self.dry_run {
            self.logger.dry_run(&format!(
                "write_file {target} (append={append}, bytes={})",
                value.len()
            ));
            return Ok(ActionOutcome::Skipped);
        }

        let start = Instant::now();
        if let Some(parent) = Path::new(&target).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create parent directories for '{target}'")
                })?;
            }
        }

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&target)
            .with_context(|| format!("failed to open '{target}' for writing"))?;

        file.write_all(value.as_bytes())
            .with_context(|| format!("failed to write to '{target}'"))?;

        self.logger
            .action_success(label, start.elapsed().as_millis() as u64);
        Ok(ActionOutcome::Success)
    }

    pub(crate) fn copy_file_action(
        &mut self,
        action: &Action,
        label: &str,
        is_bg: bool,
    ) -> Result<ActionOutcome> {
        self.ensure_not_background("copy_file", is_bg)?;

        let source = action
            .source
            .as_ref()
            .context("'copy_file' action requires a 'source' field")?;
        let destination = action
            .destination
            .as_ref()
            .context("'copy_file' action requires a 'destination' field")?;

        let source = self.substitute(source);
        let destination = self.substitute(destination);
        let overwrite = action.overwrite.unwrap_or(false);

        self.logger.action_start(
            "fs",
            label,
            &format!("copy_file {source} -> {destination} (overwrite={overwrite})"),
        );

        if self.dry_run {
            self.logger.dry_run(&format!(
                "copy_file {source} -> {destination} (overwrite={overwrite})"
            ));
            return Ok(ActionOutcome::Skipped);
        }

        let start = Instant::now();
        let source_path = Path::new(&source);
        if !source_path.is_file() {
            return Err(anyhow!("source file '{source}' does not exist or is not a file"));
        }

        let destination_path = Path::new(&destination);
        if destination_path.exists() {
            if !overwrite {
                return Err(anyhow!(
                    "destination '{destination}' already exists; set 'overwrite': true to replace it"
                ));
            }
            self.remove_existing_destination(destination_path)?;
        }

        if let Some(parent) = destination_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create destination parent directories for '{destination}'")
                })?;
            }
        }

        fs::copy(source_path, destination_path)
            .with_context(|| format!("failed to copy '{source}' to '{destination}'"))?;

        self.logger
            .action_success(label, start.elapsed().as_millis() as u64);
        Ok(ActionOutcome::Success)
    }

    pub(crate) fn move_file_action(
        &mut self,
        action: &Action,
        label: &str,
        is_bg: bool,
    ) -> Result<ActionOutcome> {
        self.ensure_not_background("move_file", is_bg)?;

        let source = action
            .source
            .as_ref()
            .context("'move_file' action requires a 'source' field")?;
        let destination = action
            .destination
            .as_ref()
            .context("'move_file' action requires a 'destination' field")?;

        let source = self.substitute(source);
        let destination = self.substitute(destination);
        let overwrite = action.overwrite.unwrap_or(false);

        self.logger.action_start(
            "fs",
            label,
            &format!("move_file {source} -> {destination} (overwrite={overwrite})"),
        );

        if self.dry_run {
            self.logger.dry_run(&format!(
                "move_file {source} -> {destination} (overwrite={overwrite})"
            ));
            return Ok(ActionOutcome::Skipped);
        }

        let start = Instant::now();
        let source_path = Path::new(&source);
        if !source_path.exists() {
            return Err(anyhow!("source path '{source}' does not exist"));
        }

        let destination_path = Path::new(&destination);
        if destination_path.exists() {
            if !overwrite {
                return Err(anyhow!(
                    "destination '{destination}' already exists; set 'overwrite': true to replace it"
                ));
            }
            self.remove_existing_destination(destination_path)?;
        }

        if let Some(parent) = destination_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create destination parent directories for '{destination}'")
                })?;
            }
        }

        if let Err(rename_err) = fs::rename(source_path, destination_path) {
            if source_path.is_file() {
                fs::copy(source_path, destination_path).with_context(|| {
                    format!(
                        "failed to move '{source}' to '{destination}' via copy fallback after rename error: {rename_err}"
                    )
                })?;
                fs::remove_file(source_path)
                    .with_context(|| format!("failed to remove source file '{source}' after copy"))?;
            } else {
                return Err(rename_err)
                    .with_context(|| format!("failed to move '{source}' to '{destination}'"));
            }
        }

        self.logger
            .action_success(label, start.elapsed().as_millis() as u64);
        Ok(ActionOutcome::Success)
    }

    pub(crate) fn remove_path_action(
        &mut self,
        action: &Action,
        label: &str,
        is_bg: bool,
    ) -> Result<ActionOutcome> {
        self.ensure_not_background("remove_path", is_bg)?;

        let path = action
            .path
            .as_ref()
            .context("'remove_path' action requires a 'path' field")?;

        let target = self.substitute(path);
        let recursive = action.recursive.unwrap_or(false);
        let ignore_missing = action.ignore_missing.unwrap_or(false);

        self.logger.action_start(
            "fs",
            label,
            &format!(
                "remove_path {target} (recursive={recursive}, ignore_missing={ignore_missing})"
            ),
        );

        if self.dry_run {
            self.logger.dry_run(&format!(
                "remove_path {target} (recursive={recursive}, ignore_missing={ignore_missing})"
            ));
            return Ok(ActionOutcome::Skipped);
        }

        let start = Instant::now();
        let target_path = Path::new(&target);

        if !target_path.exists() {
            if ignore_missing {
                self.logger
                    .action_success(label, start.elapsed().as_millis() as u64);
                return Ok(ActionOutcome::Success);
            }
            return Err(anyhow!("path '{target}' does not exist"));
        }

        if target_path.is_dir() {
            if recursive {
                fs::remove_dir_all(target_path)
                    .with_context(|| format!("failed to remove directory '{target}'"))?;
            } else {
                fs::remove_dir(target_path)
                    .with_context(|| format!("failed to remove directory '{target}' (not empty?)"))?;
            }
        } else {
            fs::remove_file(target_path)
                .with_context(|| format!("failed to remove file '{target}'"))?;
        }

        self.logger
            .action_success(label, start.elapsed().as_millis() as u64);
        Ok(ActionOutcome::Success)
    }

    fn ensure_not_background(&self, action_name: &str, is_bg: bool) -> Result<()> {
        if is_bg {
            return Err(anyhow!(
                "'{action_name}' action does not support 'background: true'"
            ));
        }
        Ok(())
    }

    fn remove_existing_destination(&self, destination_path: &Path) -> Result<()> {
        if destination_path.is_dir() {
            fs::remove_dir_all(destination_path).with_context(|| {
                format!(
                    "failed to remove existing destination directory '{}'",
                    destination_path.display()
                )
            })?;
        } else {
            fs::remove_file(destination_path).with_context(|| {
                format!(
                    "failed to remove existing destination file '{}'",
                    destination_path.display()
                )
            })?;
        }
        Ok(())
    }
}
