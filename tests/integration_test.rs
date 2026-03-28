use std::io::Write;
use std::io::{Read, Result as IoResult};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
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
    // "fail" fails but we continue, so "after" must have run and the binary
    // must exit with a non-zero code because one action failed.
    assert!(!out.status.success(), "exit code must be non-zero when an action fails");
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
    let marker = std::env::temp_dir().join("deploy_manager_wait_file_ready.txt");
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
    let marker = std::env::temp_dir().join("deploy_manager_wait_missing_file.txt");
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
    let marker = std::env::temp_dir().join("deploy_manager_wait_dry_run_file.txt");
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
