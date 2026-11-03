# Stop the lmx-server started by start-lmx.ps1 (only the PID it recorded).
$root = Split-Path -Parent $PSScriptRoot
$pidFile = Join-Path $root 'data\lmx.pid'
if (-not (Test-Path $pidFile)) { Write-Output 'no data\lmx.pid; nothing to stop'; exit 0 }
$id = [int](Get-Content $pidFile)
$proc = Get-Process -Id $id -ErrorAction SilentlyContinue
if ($proc -and $proc.ProcessName -eq 'lmx-server') { Stop-Process -Id $id -Force; Write-Output "stopped PID $id" }
else { Write-Output "PID $id is not lmx-server (already stopped)" }
Remove-Item $pidFile -ErrorAction SilentlyContinue
