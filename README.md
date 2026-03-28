# deploy-agent

CLI Rust che esegue azioni di deploy definite in un file JSON.

## Azioni supportate

- `run`: esegue un programma con argomenti opzionali
- `shell`: esegue un comando shell
- `wait`: sospende l'esecuzione per una durata fissa, finche' compare un file o finche' un endpoint HTTP risponde con successo
- `set_env`: imposta una variabile d'ambiente per le azioni successive
- `unset_env`: rimuove una variabile d'ambiente per le azioni successive
- `mkdir`: crea directory senza dipendere da `mkdir` della shell
- `write_file`: scrive o appende testo a un file
- `copy_file`: copia un file da `source` a `destination`
- `move_file`: sposta/rinomina un file da `source` a `destination`
- `remove_path`: elimina file o directory

## Esempio

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

Le action file-system permettono di ridurre i comandi shell dipendenti dal sistema operativo (`mkdir`, `cp`, `mv`, `rm`, redirection `>`).

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

Note operative:

- `mkdir`, `write_file`, `copy_file`, `move_file`, `remove_path` non supportano `background: true`
- in `copy_file` e `move_file`, se `destination` esiste serve `overwrite: true`
- `write_file` crea automaticamente le directory parent se mancanti
- in `remove_path`, `recursive` default = `false` e `ignore_missing` default = `false`

## Wait action

L'azione `wait` supporta tre modalita'.

Attesa fissa:

```json
{
  "name": "pause",
  "type": "wait",
  "duration_ms": 1000
}
```

Attesa fino alla comparsa di un file:

```json
{
  "name": "wait-for-log",
  "type": "wait",
  "until_file_exists": "./tmp/ready.log",
  "timeout_ms": 10000,
  "interval_ms": 250
}
```

Attesa fino a risposta HTTP di successo:

```json
{
  "name": "wait-for-api",
  "type": "wait",
  "until_http_ok": "http://127.0.0.1:8080/health",
  "timeout_ms": 10000,
  "interval_ms": 250
}
```

Note operative:

- bisogna specificare uno solo tra `duration_ms`, `until_file_exists` e `until_http_ok`
- per `until_file_exists` e `until_http_ok`, `timeout_ms` default = `30000` e `interval_ms` default = `200`
- `interval_ms` deve essere maggiore di `0`
- `wait` non supporta `background: true`
- con `--dry-run` la pausa viene loggata ma non eseguita

## Esecuzione

```bash
cargo run --release -- --config .\examples\deploy.json
```
