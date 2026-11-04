# lmx

Lightweight standalone rewrite of LoRA Manager: a single Rust binary (axum) serving a Vue 3 PWA,
built to manage a large local LoRA / model library from desktop and phone, with in-app Civitai
search and download. It reads and writes LoRA Manager's `<stem>.metadata.json` sidecars
byte-for-byte compatibly, so both apps can manage the same library.

License: GPL-3.0-only (derived from willmiao/ComfyUI-Lora-Manager).

**Usage guide (Japanese): [`docs/usage.md`](docs/usage.md)** — start/stop, phone install,
settings, running next to LoRA Manager, switching over and back.

## Features

- Library: fast grid (15k+ models), search, filters (base model, folder, favorites, damaged,
  no metadata, new version available), detail sheet, edit / move / rename / trash with undo,
  bulk actions, duplicates, folder watching
- Civitai: search with sorted keyword results, base model picker, favorites, URL import,
  early access / Buzz-only badges, resumable downloads that write LoRA Manager sidecars
- Metadata: bulk fetch from the local CivArchive DB first (`archive_first` / `archive_only` /
  `civitai_first`), backups before every sidecar write
- New versions of owned models (daily check), library statistics, optional password,
  settings backup

## Configure

```bash
mkdir -p data && cp lmx.example.toml data/lmx.toml   # then edit roots or point lm_settings at LoRA Manager
```

`data/` holds `lmx.toml`, the rebuildable index and caches, and lmx's own state; see the table
in `docs/usage.md`.

## Develop

```bash
cargo test                          # backend
cd web && npm install && npm test && npm run typecheck && npm run build && cd ..   # PWA -> web/dist
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

## Docs

`docs/plan.md` (plan), `docs/spike-results.md`, `docs/phase1-results.md` … `docs/phase4-results.md`
(measurements per phase), `docs/civitai-search.md` (Civitai search behaviour).

## Hash benchmark

```bash
cargo build --release -p lmx-scan --bin hashbench
./target/release/hashbench <file-list.txt> <threads> [expected.tsv]
```
