# Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
#requires -Version 5.1
<#
.SYNOPSIS
  Launch Cove Studio in debug mode from the repo root.

.DESCRIPTION
  Thin convenience wrapper around `scripts/dev.ps1`. Defaults to
  verbose backend tracing (`-LogTrace` = RUST_LOG=cove_studio=debug,info) so
  the dev window shows the same level of detail you'd expect from a
  "debug build" — without having to remember the flag.

  The actual launch logic — local tauri CLI resolution, Tauri config
  selection, Vite startup via `beforeDevCommand` — lives in
  `scripts/dev.ps1`. Edit that file, not this one, when you need to
  change the dev story.

.PARAMETER Quiet
  Skip the verbose RUST_LOG and run with whatever level is already in
  the environment (or unset). Useful when you want a clean log output.

.PARAMETER RustLog
  Override the RUST_LOG value (only relevant without -Quiet). Default:
  `cove_studio=debug,info`. Examples: `cove_studio=trace,info`, `info,cove_studio=debug,
  hyper=warn`.

.EXAMPLE
  ./run.ps1
    Launch dev mode with verbose backend trace.

.EXAMPLE
  ./run.ps1 -Quiet
    Launch dev mode without setting RUST_LOG.

.EXAMPLE
  ./run.ps1 -RustLog 'cove_studio=trace,info,hyper=warn'
    Launch dev mode with a custom trace level.
#>
[CmdletBinding()]
param(
    [switch]$Quiet,
    [string]$RustLog = 'cove_studio=debug,info'
)

$ErrorActionPreference = 'Stop'

$RepoRoot = $PSScriptRoot
Set-Location $RepoRoot

$dev = Join-Path $RepoRoot 'scripts\dev.ps1'
if (-not (Test-Path $dev)) {
    throw "scripts/dev.ps1 not found at $dev. Has the repo layout changed?"
}

if (-not $Quiet) {
    $env:RUST_LOG = $RustLog
    Write-Host "RUST_LOG=$RustLog (debug mode)" -ForegroundColor DarkGray
    # scripts/dev.ps1 -LogTrace would overwrite RUST_LOG with its own
    # default, so we set the env var here and call dev.ps1 without the
    # flag — RUST_LOG is already exported into the child process.
}

& $dev
exit $LASTEXITCODE
