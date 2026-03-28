use chrono::Local;
use colored::Colorize;

pub struct Logger {
    verbose: bool,
}

impl Logger {
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }

    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    // ── Private helpers ────────────────────────────────────────────────────

    fn ts() -> String {
        Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
    }

    // ── Public API ─────────────────────────────────────────────────────────

    /// Print the top-level banner for the deployment.
    pub fn header(&self, name: &str) {
        println!(
            "{}",
            format!("◆  Deploy Manager  —  {name}").bright_white().bold(),
        );
    }

    /// Print a section heading.
    pub fn section(&self, title: &str) {
        println!();
        println!("  {}  {}", "▶".bright_blue(), title.bright_white().bold());
    }

    /// Announce that an action is starting.
    pub fn action_start(&self, kind: &str, name: &str, detail: &str) {
        println!(
            "  {}  {}  {}  {}",
            Self::ts().dimmed(),
            "►".bright_yellow(),
            format!("[{kind}]").bright_blue(),
            format!("{name}: {detail}").white(),
        );
    }

    /// Announce successful completion of an action.
    pub fn action_success(&self, name: &str, duration_ms: u64) {
        let dur = if duration_ms > 0 {
            format!(" ({:.2}s)", duration_ms as f64 / 1000.0)
        } else {
            String::new()
        };
        println!(
            "  {}  {}  {}",
            Self::ts().dimmed(),
            "✓".bright_green(),
            format!("{name}{dur}").green(),
        );
    }

    /// Announce that an action failed.
    pub fn action_error(&self, name: &str, msg: &str) {
        eprintln!(
            "  {}  {}  {}",
            Self::ts().dimmed(),
            "✗".bright_red(),
            format!("{name}: {msg}").red(),
        );
    }

    /// Announce that an action was dispatched to the background.
    pub fn action_background(&self, name: &str) {
        println!(
            "  {}  {}  {}",
            Self::ts().dimmed(),
            "⟳".bright_magenta(),
            format!("{name} started in background").magenta(),
        );
    }

    /// Log an environment variable being set (verbose only).
    pub fn env_set(&self, key: &str, value: &str) {
        if self.verbose {
            println!(
                "  {}  {}  {}",
                Self::ts().dimmed(),
                "⚙".bright_cyan(),
                format!("env: {key}={value}").cyan(),
            );
        }
    }

    /// Log an environment variable being removed (verbose only).
    pub fn env_unset(&self, key: &str) {
        if self.verbose {
            println!(
                "  {}  {}  {}",
                Self::ts().dimmed(),
                "⚙".bright_cyan(),
                format!("env: unset {key}").cyan(),
            );
        }
    }

    /// Generic informational message.
    pub fn info(&self, msg: &str) {
        println!(
            "  {}  {}  {}",
            Self::ts().dimmed(),
            "ℹ".bright_blue(),
            msg.blue(),
        );
    }

    /// Verbose-only diagnostic message.
    pub fn verbose(&self, msg: &str) {
        if self.verbose {
            println!(
                "  {}  {}  {}",
                Self::ts().dimmed(),
                "·".dimmed(),
                msg.dimmed(),
            );
        }
    }

    /// Print a captured output line indented under the current action.
    pub fn output_line(&self, line: &str) {
        if self.verbose {
            println!("        {} {}", "│".dimmed(), line.dimmed());
        }
    }

    /// Print a dry-run notice instead of actually running something.
    pub fn dry_run(&self, msg: &str) {
        println!(
            "  {}  {}  {}",
            Self::ts().dimmed(),
            "◎".bright_yellow(),
            format!("[dry-run] {msg}").yellow(),
        );
    }

    /// Print the final summary footer.
    pub fn footer(&self, total: usize, success: usize, failed: usize, skipped: usize) {
        println!();

        let summary = format!(
            "  total: {total}  ✓ success: {success}  ✗ failed: {failed}  ↷ skipped: {skipped}"
        );
        if failed > 0 {
            summary.bright_red().to_string()
        } else {
            summary.bright_green().to_string()
        };
    }
}
