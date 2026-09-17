# Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
#requires -Version 5.1
<#
.SYNOPSIS
  Launch Cove Studio in dev mode (Svelte frontend + Tauri shell, debug build).

.DESCRIPTION
  Wraps `tauri dev` with the right config preselected and the local
  `@tauri-apps/cli` resolved out of `frontend/node_modules`. The
  beforeDevCommand in `tauri.svelte.conf.json` starts Vite on port
  5173; tauri-dev then compiles the Rust shell and opens the window.

  On a long debug build set `-LogTrace` to drop a richer trace into
  the backend output (useful when chasing citation/stream issues).

.PARAMETER LogTrace
  Set `RUST_LOG=cove_studio=debug,info` for this run so the backend emits
  debug-level traces from the `cove_studio` crate plus default info-level
  from the rest. Off by default.

.EXAMPLE
  ./scripts/dev.ps1
  ./scripts/dev.ps1 -LogTrace
#>
[CmdletBinding()]
param(
    [switch]$LogTrace
)

$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

$tauriBin = Join-Path $RepoRoot 'frontend\node_modules\.bin\tauri.CMD'
if (-not (Test-Path $tauriBin)) {
    throw "Local tauri CLI not found at $tauriBin. Run ``pnpm --dir frontend install`` first."
}

$config = 'src-tauri\tauri.svelte.conf.json'
if (-not (Test-Path $config)) {
    throw "Tauri config not found at $config. Are you running from the repo root?"
}

if ($LogTrace) {
    $env:RUST_LOG = 'cove_studio=debug,info'
    Write-Host 'RUST_LOG=cove_studio=debug,info (verbose backend trace enabled)' -ForegroundColor DarkGray
}

& $tauriBin dev --config $config
exit $LASTEXITCODE
