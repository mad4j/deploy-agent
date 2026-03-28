use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

// ── helpers ───────────────────────────────────────────────────────────────────

fn binary_path() -> std::path::PathBuf {
    let mut p = std::env::current_exe().unwrap();
    // Move from deps/<test_binary> → target/<profile>/deploy-manager
    p.pop(); // remove test binary name
    if p.ends_with("deps") {
        p.pop(); // remove "deps"
    }
    p.join("deploy-manager")
}

fn run_with_config(json: &str, extra_args: &[&str]) -> std::process::Output {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(json.as_bytes()).unwrap();
    Command::new(binary_path())
        .arg("--config")
        .arg(f.path())
        .args(extra_args)
        .output()
        .expect("failed to run deploy-manager binary")
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[test]
fn test_shell_action_success() {
    let config = r#"
    {
      "name": "test",
      "actions": [
        {
          "name": "hello",
          "type": "shell",
          "command": "echo hello"
        }
      ]
    }"#;
    let out = run_with_config(config, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn test_run_action_success() {
    let config = r#"
    {
      "name": "test",
      "actions": [
        {
          "name": "echo",
          "type": "run",
          "command": "echo",
          "args": ["hello", "world"]
        }
      ]
    }"#;
    let out = run_with_config(config, &[]);
    assert!(out.status.success());
}

#[test]
fn test_set_env_and_substitution() {
    let config = r#"
    {
      "name": "test",
      "actions": [
        { "name": "set", "type": "set_env", "key": "MY_VAR", "value": "42" },
        { "name": "use", "type": "shell",   "command": "test \"${MY_VAR}\" = \"42\"" }
      ]
    }"#;
    let out = run_with_config(config, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn test_unset_env() {
    let config = r#"
    {
      "name": "test",
      "actions": [
        { "name": "set",   "type": "set_env",   "key": "TEMP_VAR", "value": "yes" },
        { "name": "unset", "type": "unset_env", "key": "TEMP_VAR" },
        { "name": "check", "type": "shell",     "command": "test -z \"${TEMP_VAR}\"" }
      ]
    }"#;
    let out = run_with_config(config, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn test_dry_run_does_not_execute() {
    // Write a file if executed; expect NOT to exist with --dry-run.
    let marker = std::env::temp_dir().join("deploy_manager_dry_run_marker.txt");
    let _ = std::fs::remove_file(&marker);
    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{ "name": "touch", "type": "shell", "command": "touch {}" }}
          ]
        }}"#,
        marker.display()
    );
    let out = run_with_config(&config, &["--dry-run"]);
    assert!(out.status.success());
    assert!(!marker.exists(), "dry-run must not create the marker file");
}

#[test]
fn test_on_failure_continue() {
    let config = r#"
    {
      "name": "test",
      "actions": [
        { "name": "fail",   "type": "shell", "command": "false", "on_failure": "continue" },
        { "name": "after",  "type": "shell", "command": "echo still_running" }
      ]
    }"#;
    let out = run_with_config(config, &[]);
    // Even though "fail" fails, we continue, so the binary should exit with 1
    // (because there was one failure) but "after" must have run.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("still_running"),
        "stdout: {stdout}"
    );
}

#[test]
fn test_on_failure_stop_default() {
    // With default on_failure=stop the second action must NOT run.
    let config = r#"
    {
      "name": "test",
      "actions": [
        { "name": "fail",  "type": "shell", "command": "false" },
        { "name": "after", "type": "shell", "command": "echo should_not_appear" }
      ]
    }"#;
    let out = run_with_config(config, &[]);
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("should_not_appear"),
        "stdout: {stdout}"
    );
}

#[test]
fn test_background_action() {
    // A background action that writes a file after a short delay.
    let marker = std::env::temp_dir().join("deploy_manager_bg_test.txt");
    let _ = std::fs::remove_file(&marker);
    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{
              "name": "bg-write",
              "type": "shell",
              "command": "echo bg_done > {}",
              "background": true
            }}
          ]
        }}"#,
        marker.display()
    );
    let out = run_with_config(&config, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // The file must have been created once the executor joined the background
    // process.
    assert!(marker.exists(), "background process must have created the marker file");
    let _ = std::fs::remove_file(&marker);
}

#[test]
fn test_global_env_substitution() {
    let config = r#"
    {
      "name": "test",
      "env": { "GREETING": "hello" },
      "actions": [
        { "name": "greet", "type": "shell", "command": "test \"${GREETING}\" = \"hello\"" }
      ]
    }"#;
    let out = run_with_config(config, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn test_invalid_config_exits_nonzero() {
    let out = run_with_config("not valid json", &[]);
    assert!(!out.status.success());
}

#[test]
fn test_verbose_flag_accepted() {
    let config = r#"
    {
      "name": "test",
      "actions": [
        { "name": "ok", "type": "shell", "command": "echo verbose_test" }
      ]
    }"#;
    let out = run_with_config(config, &["--verbose"]);
    assert!(out.status.success());
}
