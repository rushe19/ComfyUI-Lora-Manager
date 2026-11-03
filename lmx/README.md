# lmx

Lightweight standalone rewrite of LoRA Manager: a single Rust binary (axum) serving a Vue 3 PWA,
built to manage a large local LoRA / model library from desktop and phone, with in-app Civitai
search and download. It reads and writes LoRA Manager's `<stem>.metadata.json` sidecars so both
apps can manage the same library.

Status: Phase 0 (spike) — see `docs/spike-results.md`. License: GPL-3.0-only (derived from
willmiao/ComfyUI-Lora-Manager).

## Develop

```bash
# backend
cargo test
cargo build --release -p lmx-server

# frontend (PWA)
cd web
npm install
npm test
npm run build          # -> web/dist, served by lmx-server

# run (from lmx/), defaults: 0.0.0.0:9100, web dir web/dist
./target/release/lmx-server
```

Environment: `LMX_HOST`, `LMX_PORT` (9100), `LMX_WEB_DIR` (web/dist), `LMX_LOG` (info).

## Android install (PWA)

Android only installs PWAs from a secure origin. Expose the server over Tailscale HTTPS:

```bash
tailscale serve --bg --https=10000 http://127.0.0.1:9100
# then open https://<machine>.<tailnet>.ts.net:10000/ in Chrome on the phone -> "Install app"
```

## Hash benchmark

```bash
cargo build --release -p lmx-scan --bin hashbench
./target/release/hashbench <file-list.txt> <threads> [expected.tsv]
```
