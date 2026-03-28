# deploy-manager

CLI Rust che esegue azioni di deploy definite in un file JSON.

## Azioni supportate

- `run`: esegue un programma con argomenti opzionali
- `shell`: esegue un comando shell
- `wait`: sospende l'esecuzione per una durata fissa, finche' compare un file o finche' un endpoint HTTP risponde con successo
- `set_env`: imposta una variabile d'ambiente per le azioni successive
- `unset_env`: rimuove una variabile d'ambiente per le azioni successive

## Esempio

```json
{
  "name": "my-app-deployment",
  "env": {
    "APP_NAME": "my-app"
  },
  "actions": [
    {
      "name": "announce",
      "type": "shell",
      "command": "echo Deploying ${APP_NAME}"
    },
    {
      "name": "wait-for-service",
      "type": "wait",
      "duration_ms": 1500
    },
    {
      "name": "run-check",
      "type": "run",
      "command": "echo",
      "args": ["deployment completed"]
    }
  ]
}
```

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
