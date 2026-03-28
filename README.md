# deploy-agent

Rust CLI that executes deployment actions defined in a JSON file.

## Supported actions

- `run`: executes a program with optional arguments
- `shell`: executes a shell command
- `wait`: pauses execution for a fixed duration, until a file appears, or until an HTTP endpoint responds successfully
- `set_env`: sets an environment variable for subsequent actions
- `unset_env`: removes an environment variable for subsequent actions
- `mkdir`: creates directories without relying on shell `mkdir`
- `write_file`: writes or appends text to a file
- `copy_file`: copies a file from `source` to `destination`
- `move_file`: moves/renames a file from `source` to `destination`
- `remove_path`: removes a file or directory

## Example

```json
{
  "name": "my-app-deployment",
  "env": {
    "APP_NAME": "my-app",
    "BUILD_DIR": "./tmp/build"
  },
  "actions": [
    {
      "name": "create-build-dir",
      "type": "mkdir",
      "path": "${BUILD_DIR}",
      "recursive": true
    },
    {
      "name": "write-metadata",
      "type": "write_file",
      "path": "${BUILD_DIR}/metadata.txt",
      "content": "app=${APP_NAME}\n"
    },
    {
      "name": "backup-metadata",
      "type": "copy_file",
      "source": "${BUILD_DIR}/metadata.txt",
      "destination": "${BUILD_DIR}/metadata.bak",
      "overwrite": true
    },
    {
      "name": "rename-backup",
      "type": "move_file",
      "source": "${BUILD_DIR}/metadata.bak",
      "destination": "${BUILD_DIR}/metadata.previous.txt",
      "overwrite": true
    },
    {
      "name": "wait-for-service",
      "type": "wait",
      "duration_ms": 150
    },
    {
      "name": "cleanup-build-dir",
      "type": "remove_path",
      "path": "${BUILD_DIR}",
      "recursive": true
    }
  ]
}
```

## File-system actions

File-system actions help reduce OS-dependent shell commands (`mkdir`, `cp`, `mv`, `rm`, redirection `>`).

`mkdir`

```json
{
  "name": "create-dir",
  "type": "mkdir",
  "path": "./build/output",
  "recursive": true
}
```

`write_file`

```json
{
  "name": "write-file",
  "type": "write_file",
  "path": "./build/output/version.txt",
  "content": "1.0.0\n",
  "append": false
}
```

`copy_file`

```json
{
  "name": "copy-artifact",
  "type": "copy_file",
  "source": "./build/output/app.bin",
  "destination": "./release/app.bin",
  "overwrite": true
}
```

`move_file`

```json
{
  "name": "archive-log",
  "type": "move_file",
  "source": "./logs/current.log",
  "destination": "./logs/archive/current.log",
  "overwrite": true
}
```

`remove_path`

```json
{
  "name": "cleanup",
  "type": "remove_path",
  "path": "./tmp",
  "recursive": true,
  "ignore_missing": true
}
```

Operational notes:

- `mkdir`, `write_file`, `copy_file`, `move_file`, and `remove_path` do not support `background: true`
- in `copy_file` and `move_file`, if `destination` exists, `overwrite: true` is required
- `write_file` automatically creates missing parent directories
- in `remove_path`, `recursive` default = `false` and `ignore_missing` default = `false`

## Wait action

The `wait` action supports three modes.

Fixed wait:

```json
{
  "name": "pause",
  "type": "wait",
  "duration_ms": 1000
}
```

Wait until a file appears:

```json
{
  "name": "wait-for-log",
  "type": "wait",
  "until_file_exists": "./tmp/ready.log",
  "timeout_ms": 10000,
  "interval_ms": 250
}
```

Wait until a successful HTTP response:

```json
{
  "name": "wait-for-api",
  "type": "wait",
  "until_http_ok": "http://127.0.0.1:8080/health",
  "timeout_ms": 10000,
  "interval_ms": 250
}
```

Operational notes:

- you must specify only one of `duration_ms`, `until_file_exists`, and `until_http_ok`
- for `until_file_exists` and `until_http_ok`, `timeout_ms` default = `30000` and `interval_ms` default = `200`
- `interval_ms` must be greater than `0`
- `wait` does not support `background: true`
- with `--dry-run`, the wait is logged but not executed

## Run

```bash
cargo run --release -- --config .\examples\deploy.json
```
