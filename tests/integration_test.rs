use std::io::Write;
use std::io::{Read, Result as IoResult};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::{NamedTempFile, tempdir};

// ── helpers ───────────────────────────────────────────────────────────────────

fn binary_path() -> std::path::PathBuf {
    let mut p = std::env::current_exe().unwrap();
    // Move from deps/<test_binary> -> target/<profile>/deploy-agent
    p.pop(); // remove test binary name
    if p.ends_with("deps") {
        p.pop(); // remove "deps"
    }
    p.join("deploy-agent")
}

fn run_with_config(json: &str, extra_args: &[&str]) -> std::process::Output {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(json.as_bytes()).unwrap();
    Command::new(binary_path())
        .arg("--config")
        .arg(f.path())
        .args(extra_args)
        .output()
        .expect("failed to run deploy-agent binary")
}

    fn json_path(path: &std::path::Path) -> String {
      path.display().to_string().replace('\\', "\\\\")
    }

    fn json_string(value: &str) -> String {
      serde_json::to_string(value).unwrap()
    }

    fn write_http_response(stream: &mut TcpStream, status_code: u16, body: &str) -> IoResult<()> {
      let reason = match status_code {
        200 => "OK",
        503 => "Service Unavailable",
        _ => "Unknown",
      };
      let response = format!(
        "HTTP/1.1 {status_code} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
      );
      stream.write_all(response.as_bytes())
    }

    fn handle_http_request(mut stream: TcpStream, ready: &AtomicBool) -> IoResult<bool> {
      stream.set_nonblocking(false)?;
      let mut buffer = [0u8; 1024];
      let _ = stream.read(&mut buffer);
      let is_ready = ready.load(Ordering::SeqCst);
      let status_code = if is_ready { 200 } else { 503 };
      write_http_response(&mut stream, status_code, if is_ready { "ready" } else { "waiting" })?;
      Ok(is_ready)
    }

    fn spawn_http_ready_server(
      ready_after_ms: Option<u64>,
      shutdown_after_ms: u64,
    ) -> (SocketAddr, thread::JoinHandle<()>) {
      let listener = TcpListener::bind("127.0.0.1:0").unwrap();
      listener.set_nonblocking(true).unwrap();
      let addr = listener.local_addr().unwrap();
      let ready = Arc::new(AtomicBool::new(false));

      let ready_for_timer = Arc::clone(&ready);
      let timer = ready_after_ms.map(|delay| {
        thread::spawn(move || {
          thread::sleep(Duration::from_millis(delay));
          ready_for_timer.store(true, Ordering::SeqCst);
        })
      });

      let ready_for_server = Arc::clone(&ready);
      let handle = thread::spawn(move || {
        let start = Instant::now();
        loop {
          if start.elapsed() > Duration::from_millis(shutdown_after_ms) {
            break;
          }

          match listener.accept() {
            Ok((stream, _)) => {
              let is_ready = handle_http_request(stream, &ready_for_server).unwrap();
              if is_ready {
                break;
              }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
              thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("HTTP test server accept failed: {err}"),
          }
        }

        if let Some(timer) = timer {
          timer.join().unwrap();
        }
      });

      (addr, handle)
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
    let out = run_with_config(&config, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn test_run_action_success() {
    let cmd_json = json_string(&json_path(&binary_path()));
    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{
              "name": "version",
              "type": "run",
              "command": {cmd_json},
              "args": ["--version"]
            }}
          ]
        }}"#
    );
    let out = run_with_config(&config, &[]);
    assert!(out.status.success());
}

#[test]
fn test_set_env_and_substitution() {
    let workspace = tempdir().unwrap();
    let out_file = workspace.path().join("set_env_value.txt");
    let out_file_json = json_string(&json_path(&out_file));

    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{ "name": "set", "type": "set_env", "key": "MY_VAR", "value": "42" }},
            {{ "name": "use", "type": "write_file", "path": {out_file_json}, "content": "${{MY_VAR}}" }}
          ]
        }}"#
    );
    let out = run_with_config(&config, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let content = std::fs::read_to_string(&out_file).unwrap();
    assert_eq!(content, "42");
}

#[test]
fn test_unset_env() {
    let workspace = tempdir().unwrap();
    let out_file = workspace.path().join("unset_env_value.txt");
    let out_file_json = json_string(&json_path(&out_file));
    let key = "DEPLOY_AGENT_TEST_TEMP_VAR_12345";

    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{ "name": "set", "type": "set_env", "key": "{key}", "value": "yes" }},
            {{ "name": "unset", "type": "unset_env", "key": "{key}" }},
            {{ "name": "check", "type": "write_file", "path": {out_file_json}, "content": "${{{key}}}" }}
          ]
        }}"#
    );
    let out = run_with_config(&config, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let content = std::fs::read_to_string(&out_file).unwrap();
    assert_eq!(content, format!("${{{key}}}"));
}

#[test]
fn test_dry_run_does_not_execute() {
    // Write a file if executed; expect NOT to exist with --dry-run.
    let marker = std::env::temp_dir().join("deploy_agent_dry_run_marker.txt");
    let _ = std::fs::remove_file(&marker);
    let marker_json = json_string(&json_path(&marker));
    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{ "name": "write", "type": "write_file", "path": {marker_json}, "content": "dry-run" }}
          ]
        }}"#
    );
    let out = run_with_config(&config, &["--dry-run"]);
    assert!(out.status.success());
    assert!(!marker.exists(), "dry-run must not create the marker file");
}

#[test]
fn test_on_failure_continue() {
    let workspace = tempdir().unwrap();
    let marker = workspace.path().join("continue_marker.txt");
    let marker_json = json_string(&json_path(&marker));
    let missing_source_json = json_string(&json_path(&workspace.path().join("missing-source.txt")));

    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{
              "name": "fail",
              "type": "copy_file",
              "source": {missing_source_json},
              "destination": {marker_json},
              "on_failure": "continue"
            }},
            {{ "name": "after", "type": "write_file", "path": {marker_json}, "content": "still_running" }}
          ]
        }}"#
    );
    let out = run_with_config(&config, &[]);
    // "fail" fails but we continue, so "after" must have run and the binary
    // must exit with a non-zero code because one action failed.
    assert!(!out.status.success(), "exit code must be non-zero when an action fails");
    assert!(marker.exists(), "follow-up action should still run with on_failure=continue");
    let content = std::fs::read_to_string(&marker).unwrap();
    assert_eq!(content, "still_running");
}

#[test]
fn test_on_failure_stop_default() {
    // With default on_failure=stop the second action must NOT run.
    let workspace = tempdir().unwrap();
    let marker = workspace.path().join("stop_marker.txt");
    let marker_json = json_string(&json_path(&marker));
    let missing_source_json = json_string(&json_path(&workspace.path().join("missing-source.txt")));

    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{
              "name": "fail",
              "type": "copy_file",
              "source": {missing_source_json},
              "destination": {marker_json}
            }},
            {{ "name": "after", "type": "write_file", "path": {marker_json}, "content": "should_not_appear" }}
          ]
        }}"#
    );
    let out = run_with_config(&config, &[]);
    assert!(!out.status.success());
    assert!(!marker.exists(), "default on_failure=stop must skip the following action");
}

#[test]
fn test_background_action() {
    // Run a child deploy-agent process in background and wait for its marker.
    let marker = std::env::temp_dir().join("deploy_agent_bg_test.txt");
    let _ = std::fs::remove_file(&marker);
    let marker_json = json_string(&json_path(&marker));

    let mut child_config_file = NamedTempFile::new().unwrap();
    let child_config = format!(
        r#"{{
          "name": "child",
          "actions": [
            {{ "name": "pause", "type": "wait", "duration_ms": 150 }},
            {{ "name": "write", "type": "write_file", "path": {marker_json}, "content": "bg_done" }}
          ]
        }}"#
    );
    child_config_file.write_all(child_config.as_bytes()).unwrap();
    let child_config_path_json = json_string(&json_path(child_config_file.path()));
    let cmd_json = json_string(&json_path(&binary_path()));

    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{
              "name": "bg-child",
              "type": "run",
              "command": {cmd_json},
              "args": ["--config", {child_config_path_json}],
              "background": true
            }},
            {{
              "name": "wait-for-child",
              "type": "wait",
              "until_file_exists": {marker_json},
              "timeout_ms": 5000,
              "interval_ms": 50
            }}
          ]
        }}"#
    );

    let out = run_with_config(&config, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(marker.exists(), "background process must have created the marker file");
    let _ = std::fs::remove_file(&marker);
}

#[test]
fn test_global_env_substitution() {
    let workspace = tempdir().unwrap();
    let out_file = workspace.path().join("global_env_value.txt");
    let out_file_json = json_string(&json_path(&out_file));

    let config = format!(
        r#"{{
          "name": "test",
          "env": {{ "GREETING": "hello" }},
          "actions": [
            {{ "name": "greet", "type": "write_file", "path": {out_file_json}, "content": "${{GREETING}}" }}
          ]
        }}"#
    );
    let out = run_with_config(&config, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let content = std::fs::read_to_string(&out_file).unwrap();
    assert_eq!(content, "hello");
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
        { "name": "ok", "type": "wait", "duration_ms": 1 }
      ]
    }"#;
    let out = run_with_config(config, &["--verbose"]);
    assert!(out.status.success());
}

#[test]
fn test_wait_action_success() {
    let config = r#"
    {
      "name": "test",
      "actions": [
        { "name": "pause", "type": "wait", "duration_ms": 60 }
      ]
    }"#;

    let start = Instant::now();
    let out = run_with_config(config, &[]);
    let elapsed = start.elapsed();

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(
        elapsed >= Duration::from_millis(40),
        "wait action returned too quickly: {elapsed:?}"
    );
}

#[test]
fn test_wait_action_dry_run_skips_delay() {
    let config = r#"
    {
      "name": "test",
      "actions": [
        { "name": "pause", "type": "wait", "duration_ms": 3000 }
      ]
    }"#;

    let start = Instant::now();
    let out = run_with_config(config, &["--dry-run"]);
    let elapsed = start.elapsed();

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(
      elapsed < Duration::from_millis(2000),
      "dry-run wait should not sleep for the requested duration: {elapsed:?}"
    );
}

#[test]
fn test_wait_action_requires_duration() {
    let config = r#"
    {
      "name": "test",
      "actions": [
        { "name": "pause", "type": "wait" }
      ]
    }"#;

    let out = run_with_config(config, &[]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
  assert!(stderr.contains("requires exactly one"), "stderr: {stderr}");
}

#[test]
fn test_wait_until_file_exists_success() {
    let marker = std::env::temp_dir().join("deploy_agent_wait_file_ready.txt");
    let _ = std::fs::remove_file(&marker);
    let marker_json = json_path(&marker);
    let marker_value_json = json_string(&marker_json);
    let marker_for_thread = marker.clone();

    let writer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(150));
        std::fs::write(&marker_for_thread, "ready").unwrap();
    });

    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{
              "name": "wait-marker",
              "type": "wait",
              "until_file_exists": {marker_value_json},
              "timeout_ms": 2000,
              "interval_ms": 50
            }}
          ]
        }}"#
    );

    let out = run_with_config(&config, &[]);
    writer.join().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(marker.exists(), "wait should complete only after the marker exists");
    let _ = std::fs::remove_file(&marker);
}

#[test]
fn test_wait_until_file_exists_times_out() {
    let marker = std::env::temp_dir().join("deploy_agent_wait_missing_file.txt");
    let _ = std::fs::remove_file(&marker);
    let marker_json = json_path(&marker);
    let marker_value_json = json_string(&marker_json);

    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{
              "name": "wait-marker",
              "type": "wait",
              "until_file_exists": {marker_value_json},
              "timeout_ms": 120,
              "interval_ms": 20
            }}
          ]
        }}"#
    );

    let out = run_with_config(&config, &[]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("timed out"), "stderr: {stderr}");
}

#[test]
fn test_wait_until_file_exists_dry_run_skips_polling() {
    let marker = std::env::temp_dir().join("deploy_agent_wait_dry_run_file.txt");
    let _ = std::fs::remove_file(&marker);
    let marker_json = json_path(&marker);
    let marker_value_json = json_string(&marker_json);

    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{
              "name": "wait-marker",
              "type": "wait",
              "until_file_exists": {marker_value_json},
              "timeout_ms": 4000,
              "interval_ms": 50
            }}
          ]
        }}"#
    );

    let start = Instant::now();
    let out = run_with_config(&config, &["--dry-run"]);
    let elapsed = start.elapsed();

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(
      elapsed < Duration::from_millis(2500),
        "dry-run file wait should not poll until timeout: {elapsed:?}"
    );
    assert!(!marker.exists(), "dry-run must not create the marker file");
}

#[test]
fn test_wait_until_http_ok_success() {
  let (addr, server) = spawn_http_ready_server(Some(150), 2000);
    let url_json = json_string(&format!("http://{addr}/health"));

    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{
              "name": "wait-http",
              "type": "wait",
              "until_http_ok": {url_json},
              "timeout_ms": 2000,
              "interval_ms": 50
            }}
          ]
        }}"#
    );

    let out = run_with_config(&config, &[]);
    server.join().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn test_wait_until_http_ok_times_out() {
  let (addr, server) = spawn_http_ready_server(None, 500);
    let url_json = json_string(&format!("http://{addr}/health"));

    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{
              "name": "wait-http",
              "type": "wait",
              "until_http_ok": {url_json},
              "timeout_ms": 150,
              "interval_ms": 30
            }}
          ]
        }}"#
    );

    let out = run_with_config(&config, &[]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("HTTP OK"), "stderr: {stderr}");
    drop(out);
    server.join().unwrap();
}

#[test]
fn test_wait_until_http_ok_dry_run_skips_polling() {
    let url_json = json_string("http://127.0.0.1:9/health");
    let config = format!(
        r#"{{
          "name": "test",
          "actions": [
            {{
              "name": "wait-http",
              "type": "wait",
              "until_http_ok": {url_json},
              "timeout_ms": 4000,
              "interval_ms": 50
            }}
          ]
        }}"#
    );

    let start = Instant::now();
    let out = run_with_config(&config, &["--dry-run"]);
    let elapsed = start.elapsed();

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(
      elapsed < Duration::from_millis(2500),
        "dry-run HTTP wait should not poll until timeout: {elapsed:?}"
    );
}

#[test]
fn test_filesystem_actions_success() {
    let workspace = tempdir().unwrap();
    let root_json = json_string(&json_path(workspace.path()));
    let source = workspace.path().join("work").join("source.txt");
    let source_json = json_string(&json_path(&source));
    let copy = workspace.path().join("work").join("copy.txt");
    let copy_json = json_string(&json_path(&copy));
    let moved = workspace.path().join("work").join("moved.txt");
    let moved_json = json_string(&json_path(&moved));

    let config = format!(
        r#"{{
          "name": "test-fs",
          "actions": [
            {{ "name": "mkdir", "type": "mkdir", "path": "${{ROOT}}/work" }},
            {{ "name": "write", "type": "write_file", "path": {source_json}, "content": "hello-${{APP}}" }},
            {{ "name": "copy", "type": "copy_file", "source": {source_json}, "destination": {copy_json} }},
            {{ "name": "move", "type": "move_file", "source": {copy_json}, "destination": {moved_json} }},
            {{ "name": "remove-source", "type": "remove_path", "path": {source_json} }}
          ],
          "env": {{
            "ROOT": {root_json},
            "APP": "demo"
          }}
        }}"#
    );

    let out = run_with_config(&config, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!source.exists(), "source should have been removed");
    assert!(moved.exists(), "moved file should exist");
    let text = std::fs::read_to_string(&moved).unwrap();
    assert_eq!(text, "hello-demo");
}

#[test]
fn test_copy_file_requires_overwrite_when_destination_exists() {
    let workspace = tempdir().unwrap();
    let source = workspace.path().join("source.txt");
    let destination = workspace.path().join("destination.txt");
    std::fs::write(&source, "v1").unwrap();
    std::fs::write(&destination, "v2").unwrap();
    let source_json = json_string(&json_path(&source));
    let destination_json = json_string(&json_path(&destination));

    let config = format!(
        r#"{{
          "name": "test-fs-overwrite",
          "actions": [
            {{ "name": "copy", "type": "copy_file", "source": {source_json}, "destination": {destination_json} }}
          ]
        }}"#
    );

    let out = run_with_config(&config, &[]);
    assert!(!out.status.success(), "copy without overwrite should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("overwrite"), "stderr: {stderr}");
    let destination_text = std::fs::read_to_string(&destination).unwrap();
    assert_eq!(destination_text, "v2");
}

#[test]
fn test_write_file_dry_run_does_not_create_file() {
    let workspace = tempdir().unwrap();
    let target = workspace.path().join("nested").join("dry-run.txt");
    let target_json = json_string(&json_path(&target));

    let config = format!(
        r#"{{
          "name": "test-fs-dry-run",
          "actions": [
            {{ "name": "write", "type": "write_file", "path": {target_json}, "content": "dry-run" }}
          ]
        }}"#
    );

    let out = run_with_config(&config, &["--dry-run"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!target.exists(), "dry-run must not create target file");
}
