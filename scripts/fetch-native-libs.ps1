# Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
#requires -Version 5.1
<#
.SYNOPSIS
  Download the native runtime DLLs Cove Studio depends on (onnxruntime
  and pdfium) for Windows x86_64 and ARM64, and place them under
  `libs/<lib>/win-<arch>/`.

.DESCRIPTION
  Pins to exact versions so the binary the user installs is the one
  every build was tested against:

    onnxruntime  1.20.0     (matches `ort = "=2.0.0-rc.9"` in Cargo.toml
                             — anything else deadlocks try_new silently
                             per libs/onnxruntime/README.md)
    pdfium       chromium/7834
                            (recent bblanchon release at the time the
                             script was written; pdfium-render 0.8+ binds
                             dynamically through the stable FPDF_ C API,
                             so newer pdfium releases work too — bump the
                             tag in $PdfiumTag if you want to track the
                             latest)

  Idempotent: existing DLLs are kept unless `-Force` is passed.

.PARAMETER Arch
  Which architecture(s) to fetch. Defaults to `both`.

.PARAMETER Force
  Re-download and overwrite even if the target DLL already exists.

.PARAMETER Onnxruntime
  Skip pdfium; only fetch onnxruntime.

.PARAMETER Pdfium
  Skip onnxruntime; only fetch pdfium.

.EXAMPLE
  ./scripts/fetch-native-libs.ps1
  ./scripts/fetch-native-libs.ps1 -Arch arm64
  ./scripts/fetch-native-libs.ps1 -Force
#>
[CmdletBinding()]
param(
    [ValidateSet('x64', 'arm64', 'both')]
    [string]$Arch = 'both',
    [switch]$Force,
    [switch]$Onnxruntime,
    [switch]$Pdfium
)

$ErrorActionPreference = 'Stop'

# Pinned versions — keep in sync with Cargo.toml / HISTORY.md.
$OnnxVersion = '1.20.0'
$PdfiumTag   = 'chromium/7834'

$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

# tar.exe ships with Windows 10 (1803+) / Windows 11 — used to unpack
# pdfium's .tgz tarballs. Bail early if missing rather than 30 s into
# the download.
$tarBin = Get-Command tar.exe -ErrorAction SilentlyContinue
if (-not $tarBin) {
    throw 'tar.exe not found on PATH. Install Git for Windows or the Windows 10 1803+ built-in tar.'
}

# When both -Onnxruntime and -Pdfium are off (the default), do both.
# When only one is set, do only that one.
$doOnnx   = (-not $Pdfium) -or $Onnxruntime
$doPdfium = (-not $Onnxruntime) -or $Pdfium

$arches = if ($Arch -eq 'both') { @('x64', 'arm64') } else { @($Arch) }

$work = Join-Path $env:TEMP ("covestudio-fetch-{0}" -f ([guid]::NewGuid().ToString('N').Substring(0, 8)))
New-Item -ItemType Directory -Path $work -Force | Out-Null
Write-Host "Scratch dir: $work" -ForegroundColor DarkGray

function Save-Download {
    param([string]$Url, [string]$Dest)
    Write-Host "  GET $Url" -ForegroundColor DarkGray
    # Stable UA + TLS 1.2: GitHub redirects sometimes 403 the PS default UA.
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $Url -OutFile $Dest -UseBasicParsing -UserAgent 'covestudio-fetch/1.0'
}

function Move-Atomic {
    param([string]$Src, [string]$Dest)
    $destDir = Split-Path -Parent $Dest
    if (-not (Test-Path $destDir)) { New-Item -ItemType Directory -Path $destDir -Force | Out-Null }
    Copy-Item -Path $Src -Destination $Dest -Force
    $size = [math]::Round((Get-Item $Dest).Length / 1MB, 2)
    Write-Host ("  -> {0}  ({1} MB)" -f $Dest, $size) -ForegroundColor Green
}

# ── onnxruntime ─────────────────────────────────────────────────────
if ($doOnnx) {
    foreach ($a in $arches) {
        $dllPath = "libs/onnxruntime/win-$a/onnxruntime.dll"
        if ((-not $Force) -and (Test-Path $dllPath)) {
            Write-Host "onnxruntime/win-$a already present (skip; -Force to redownload)" -ForegroundColor DarkGray
            continue
        }
        Write-Host "=== onnxruntime $OnnxVersion / win-$a ===" -ForegroundColor Cyan
        $url = "https://github.com/microsoft/onnxruntime/releases/download/v$OnnxVersion/onnxruntime-win-$a-$OnnxVersion.zip"
        $zip = Join-Path $work "onnx-$a.zip"
        Save-Download -Url $url -Dest $zip
        $extract = Join-Path $work "onnx-$a"
        Expand-Archive -Path $zip -DestinationPath $extract -Force
        # Archive contains a single top-level dir with `lib/onnxruntime.dll`.
        $src = Get-ChildItem -Path $extract -Recurse -Filter 'onnxruntime.dll' -File |
               Where-Object { $_.FullName -match '\\lib\\onnxruntime\.dll$' } |
               Select-Object -First 1
        if (-not $src) {
            throw "onnxruntime.dll not found inside $zip — archive layout changed?"
        }
        Move-Atomic -Src $src.FullName -Dest $dllPath
    }
}

# ── pdfium (bblanchon prebuilt) ─────────────────────────────────────
if ($doPdfium) {
    $tagEnc = [Uri]::EscapeDataString($PdfiumTag) # 'chromium/7834' -> 'chromium%2F7834'
    foreach ($a in $arches) {
        $dllPath = "libs/pdfium/win-$a/pdfium.dll"
        if ((-not $Force) -and (Test-Path $dllPath)) {
            Write-Host "pdfium/win-$a already present (skip; -Force to redownload)" -ForegroundColor DarkGray
            continue
        }
        Write-Host "=== pdfium $PdfiumTag / win-$a ===" -ForegroundColor Cyan
        $url = "https://github.com/bblanchon/pdfium-binaries/releases/download/$tagEnc/pdfium-win-$a.tgz"
        $tgz = Join-Path $work "pdfium-$a.tgz"
        Save-Download -Url $url -Dest $tgz
        $extract = Join-Path $work "pdfium-$a"
        New-Item -ItemType Directory -Path $extract -Force | Out-Null
        # tar.exe handles .tgz natively (gzip-then-tar) on Windows 10+.
        tar.exe -xzf $tgz -C $extract
        if ($LASTEXITCODE -ne 0) { throw "tar failed unpacking $tgz" }
        # bblanchon archives put the DLL at `bin/pdfium.dll`.
        $src = Get-ChildItem -Path $extract -Recurse -Filter 'pdfium.dll' -File |
               Where-Object { $_.FullName -match '\\bin\\pdfium\.dll$' } |
               Select-Object -First 1
        if (-not $src) {
            throw "pdfium.dll not found inside $tgz — archive layout changed?"
        }
        Move-Atomic -Src $src.FullName -Dest $dllPath
    }
}

# Cleanup scratch dir — keep it on failure for debugging.
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue

Write-Host ''
Write-Host '=== Done ===' -ForegroundColor Green
Get-ChildItem -Path libs -Recurse -Filter '*.dll' -File |
    Sort-Object FullName |
    Format-Table @{n='Path';e={$_.FullName.Replace($RepoRoot, '').TrimStart('\')}},
                 @{n='Size (MB)';e={[math]::Round($_.Length / 1MB, 2)}} -AutoSize
