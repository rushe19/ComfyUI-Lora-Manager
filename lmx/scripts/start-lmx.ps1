# Start lmx-server detached (survives the terminal), logs appended under data/.
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\start-lmx.ps1 [-Port 9100]
param([int]$Port = 9100)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\release\lmx-server.exe'
if (-not (Test-Path $exe)) { throw "build first: cargo build --release -p lmx-server ($exe not found)" }
$listen = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
if ($listen) { Write-Output "port $Port already in use by PID $($listen.OwningProcess)"; exit 1 }
$data = Join-Path $root 'data'
New-Item -ItemType Directory -Force -Path $data | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$env:LMX_PORT = "$Port"
$p = Start-Process -FilePath $exe -WorkingDirectory $root -WindowStyle Hidden -PassThru `
  -RedirectStandardOutput (Join-Path $data "server-$stamp.out.log") -RedirectStandardError (Join-Path $data "server-$stamp.err.log")
Set-Content -Path (Join-Path $data 'lmx.pid') -Value $p.Id
Write-Output "lmx started: PID $($p.Id), http://127.0.0.1:$Port/"
