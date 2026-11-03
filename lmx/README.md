# lmx

Lightweight standalone rewrite of LoRA Manager: a single Rust binary (axum) serving a Vue 3 PWA,
built to manage a large local LoRA / model library from desktop and phone, with in-app Civitai
search and download (Phase 2). It reads LoRA Manager's `<stem>.metadata.json` sidecars so both
apps can manage the same library.

Status: Phase 1 (read-only library browser) — see `docs/plan.md`, `docs/spike-results.md`,
`docs/phase1-results.md`. License: GPL-3.0-only (derived from willmiao/ComfyUI-Lora-Manager).

## Configure

```bash
mkdir -p data && cp lmx.example.toml data/lmx.toml   # then edit roots or point lm_settings at LoRA Manager
```

`data/` holds `lmx.toml`, the index (`index.sqlite`, rebuildable cache), thumbnails and logs.

## Develop

```bash
cargo test                          # backend
cd web && npm install && npm test && npm run build && cd ..   # PWA -> web/dist
cargo build --release -p lmx-server
```

## Run (Windows)

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\start-lmx.ps1   # detached, logs in data\
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\stop-lmx.ps1
```

Environment: `LMX_HOST` (0.0.0.0), `LMX_PORT` (9100), `LMX_WEB_DIR` (web/dist), `LMX_DATA` (data), `LMX_LOG` (info).

## Android install (PWA)

Android only installs PWAs from a secure origin. Expose the server over Tailscale HTTPS:

```bash
tailscale serve --bg --https=10000 http://127.0.0.1:9100
# open https://<machine>.<tailnet>.ts.net:10000/ in Chrome on the phone -> menu -> "Install app"
```

## Hash benchmark

```bash
cargo build --release -p lmx-scan --bin hashbench
./target/release/hashbench <file-list.txt> <threads> [expected.tsv]
```
