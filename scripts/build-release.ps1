# Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
#requires -Version 5.1
<#
.SYNOPSIS
  Build Cove Studio release MSI installers for Windows x86_64 and ARM64,
  bundle the matching native DLLs (onnxruntime + pdfium), and collect
  the artefacts under ./dist/.

.DESCRIPTION
  Pipeline per target architecture:
    1. Make sure the native runtime DLLs for the target arch exist by
       running scripts/fetch-native-libs.ps1 (idempotent — skips DLLs
       already on disk).
    2. Hand-craft a per-arch JSON overlay for `bundle.resources` that
       names only the matching arch's DLLs and write it under
       target/.tauri-overlay-<arch>.json.
    3. Invoke `tauri build --bundles msi --target <triple>` with both
       the base config and the overlay so the WiX bundler picks up the
       correct DLLs (and only those, not double-sized cross-arch).
    4. Copy the produced .msi from
       `target/<triple>/release/bundle/msi/` into ./dist/, renaming
       with a short `_x64` / `_arm64` suffix to keep both architectures
       coexisting.

  No manual operator step needed: a clean checkout can do
  `pnpm --dir frontend install` then `./scripts/build-release.ps1` and
  end up with two MSIs in dist/, each carrying the right native DLLs
  next to the binary.

  Cross-toolchain: each build is launched through the matching
  `VsDevCmd.bat -arch=<target> -host_arch=<host>` so the right
  `cl.exe` / `link.exe` are on PATH regardless of how the caller's
  shell was opened. The script auto-discovers Visual Studio via
  `vswhere.exe` and fails fast with the exact workload id if the
  required MSVC component isn't installed.

.PARAMETER Target
  Which architecture(s) to build. Defaults to `both`.
.PARAMETER Clean
  Wipe `target/<triple>/release/bundle/` before each build.
.PARAMETER FrontendInstall
  Run `pnpm --dir frontend install --frozen-lockfile` before building.
  Useful in CI; in a developer checkout it's usually redundant.
.PARAMETER SkipNativeLibs
  Skip the auto fetch-native-libs.ps1 invocation. Useful if you have
  already pinned a custom DLL build into libs/ that you don't want
  overwritten.

.EXAMPLE
  ./scripts/build-release.ps1                          # both, fresh DLLs as needed
  ./scripts/build-release.ps1 -Target x64 -Clean
  ./scripts/build-release.ps1 -Target arm64 -SkipNativeLibs
#>
[CmdletBinding()]
param(
    [ValidateSet('x64', 'arm64', 'both')]
    [string]$Target = 'both',
    [switch]$Clean,
    [switch]$FrontendInstall,
    [switch]$SkipNativeLibs,
    # Sign the application binary and the MSI with Azure Trusted
    # Signing (see scripts/sign-windows.ps1). Requires `az login`.
    # Without this switch the build produces unsigned artefacts.
    [switch]$Sign
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

$distDir = Join-Path $RepoRoot 'dist'
if (-not (Test-Path $distDir)) {
    New-Item -ItemType Directory -Path $distDir -Force | Out-Null
}

if ($FrontendInstall) {
    Write-Host '=== pnpm install (frontend) ===' -ForegroundColor Cyan
    pnpm --dir frontend install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { throw "pnpm install failed (exit $LASTEXITCODE)" }
}

$tripleByArch = @{
    'x64'   = 'x86_64-pc-windows-msvc'
    'arm64' = 'aarch64-pc-windows-msvc'
}
# VsDevCmd's `-arch` argument name (x64 is 'amd64' in VsDevCmd-speak).
$vsArchByArch = @{
    'x64'   = 'amd64'
    'arm64' = 'arm64'
}
# The MSVC workload that ships the C/C++ build tools for each target.
# vswhere will reject a VS install missing the required component, so we
# surface a clear error pointing at the VS Installer instead of letting
# link.exe fail with an opaque "machine type" message later.
$workloadByArch = @{
    'x64'   = 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64'
    'arm64' = 'Microsoft.VisualStudio.Component.VC.Tools.ARM64'
}
$archesToBuild = if ($Target -eq 'both') { @('x64', 'arm64') } else { @($Target) }

function Get-HostArch {
    switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture) {
        'Arm64' { return 'arm64' }
        'X64'   { return 'amd64' }
        'X86'   { return 'x86' }
        default { throw "Unsupported host architecture: $_" }
    }
}

function Get-VsInstallPath {
    [CmdletBinding()]
    param([string[]]$Requires)
    $vswhere = @(
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\Installer\vswhere.exe"
    ) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (-not $vswhere) {
        throw 'vswhere.exe not found. Install Visual Studio Build Tools 2022 (or newer) from https://visualstudio.microsoft.com/downloads/.'
    }
    $vswhereArgs = @('-latest', '-products', '*', '-property', 'installationPath')
    foreach ($req in $Requires) { $vswhereArgs += @('-requires', $req) }
    $path = & $vswhere @vswhereArgs
    if (-not $path) {
        throw ("No Visual Studio installation found with required component(s): {0}. " +
               'Open Visual Studio Installer and add the missing workload.') -f ($Requires -join ', ')
    }
    return ($path | Select-Object -First 1).Trim()
}

# Pre-flight: every requested arch must have its rustc target installed.
$installedTargets = (rustup target list --installed) -split "`n" | ForEach-Object { $_.Trim() }
foreach ($arch in $archesToBuild) {
    $triple = $tripleByArch[$arch]
    if ($installedTargets -notcontains $triple) {
        throw "Rust target $triple is not installed. Run ``rustup target add $triple``."
    }
}

# Auto-fetch native libs for any requested arch that doesn't have them.
if (-not $SkipNativeLibs) {
    $needsFetch = @()
    foreach ($arch in $archesToBuild) {
        $onnxDll   = "libs\onnxruntime\win-$arch\onnxruntime.dll"
        $pdfiumDll = "libs\pdfium\win-$arch\pdfium.dll"
        if (-not (Test-Path $onnxDll) -or -not (Test-Path $pdfiumDll)) {
            $needsFetch += $arch
        }
    }
    if ($needsFetch.Count -gt 0) {
        Write-Host ('=== Fetching native DLLs for: {0} ===' -f ($needsFetch -join ', ')) -ForegroundColor Cyan
        foreach ($arch in $needsFetch) {
            & (Join-Path $PSScriptRoot 'fetch-native-libs.ps1') -Arch $arch
            if ($LASTEXITCODE -ne 0) {
                throw "fetch-native-libs.ps1 failed for $arch (exit $LASTEXITCODE)"
            }
        }
    } else {
        Write-Host 'Native DLLs already present for all requested arches.' -ForegroundColor DarkGray
    }
}

# Per-arch bundle.resources overlay. Tauri 2 takes --config multiple
# times; the second --config overlays the first. Writing the overlay
# to a file avoids any cmd/powershell quote-escaping pitfalls.
$overlayDir = Join-Path $RepoRoot 'target'
if (-not (Test-Path $overlayDir)) {
    New-Item -ItemType Directory -Path $overlayDir -Force | Out-Null
}
function New-ResourcesOverlay {
    param([string]$Arch)
    # Map syntax: "<source>" -> "<dest-relative-to-resources/>".
    # Tauri resolves the SOURCE side relative to the directory of the
    # config file (i.e. src-tauri/), so we walk one level up with `../`
    # to reach the repo root where libs/ actually lives. The DEST side
    # is relative to the install's `resources/` folder — the Rust
    # loaders look for `<exe_dir>/resources/libs/<lib>/win-<arch>/<dll>`,
    # which is exactly what we ask the WiX MSI bundler to produce.
    #
    # The empty `beforeBuildCommand` is essential: this overlay is used
    # by the *bundle* phase, which runs after cargo has already produced
    # cove-studio.exe and after we have swept the stray provider DLLs.
    # The default `pnpm --dir ./frontend build` would write fresh
    # timestamps into frontend/dist, invalidating cove-studio's cargo
    # fingerprint, triggering a full re-link, triggering ort's
    # copy-dylibs step again, re-emitting the same 617 MB CUDA EP DLL
    # we just deleted. Silencing pnpm here keeps cargo a true no-op so
    # the swept DLLs stay swept and the bundler ships a lean MSI.
    # Bundle JSON-driven config registries alongside the binary.
    # The runtime `crate::presets::*_dir` resolvers walk ancestors of
    # the executable looking for `config/<dir>/`, so dropping the
    # bundled JSON at `<install>/config/...` lets the installed app
    # find them without an env-var override. The walker happily
    # recurses into per-domain subfolders (workflow-presets/legal/,
    # workflow-presets/insurance/, …) — the glob source preserves
    # the relative subpath under each root. `corpora-plugins/` is
    # deliberately excluded: the plugin system reads its own dir
    # via a separate resolver and shouldn't conflict, and pre-built
    # installs don't ship third-party plugins anyway.
    $resources = [ordered]@{
        ("../libs/pdfium/win-$Arch/pdfium.dll")             = "libs/pdfium/win-$Arch/pdfium.dll"
        ("../libs/onnxruntime/win-$Arch/onnxruntime.dll")   = "libs/onnxruntime/win-$Arch/onnxruntime.dll"
        "../config/workflow-presets/**/*.json"              = "config/workflow-presets/"
        "../config/column-presets/**/*.json"                = "config/column-presets/"
        "../config/docx-templates/**/*.json"                = "config/docx-templates/"
        "../config/model.json"                              = "config/model.json"
        # Local models catalogue for the secure local mode
        # (crate::presets::local_models).
        "../config/local-models/*.json"                     = "config/local-models/"
        # Domain-aware system-prompt prologue (v0.4.0). Six locale
        # sub-folders × 11 domains = 66 Markdown files; the Rust
        # loader in crate::presets::system_prompt walks
        # <install>/config/system-prompts/<locale>/<domain>.md with
        # locale fallback chain (requested → it → en → None).
        #
        # One glob per locale (instead of a single `**/*.md` mapped to
        # `config/system-prompts/`) because the WiX bundler flattens a
        # `**`-glob into the destination directory: every locale's
        # `pa.md` / `legal.md` / `medical.md` collided in a single dir
        # and `light.exe` failed with ICE30 ("two different components
        # install the same target file"). Mapping each locale to its
        # own destination subdir preserves the `<locale>/<domain>.md`
        # layout the Rust loader expects.
        # Base assistant instructions (loaded by
        # crate::presets::system_prompt::base_instructions).
        "../config/system-prompts/base.md"                  = "config/system-prompts/base.md"
        "../config/system-prompts/it/*.md"                  = "config/system-prompts/it/"
        "../config/system-prompts/en/*.md"                  = "config/system-prompts/en/"
        "../config/system-prompts/fr/*.md"                  = "config/system-prompts/fr/"
        "../config/system-prompts/de/*.md"                  = "config/system-prompts/de/"
        "../config/system-prompts/es/*.md"                  = "config/system-prompts/es/"
        "../config/system-prompts/pt/*.md"                  = "config/system-prompts/pt/"
    }
    $obj = @{
        build  = @{ beforeBuildCommand = '' }
        bundle = @{ resources = $resources }
    }
    $path = Join-Path $overlayDir ("tauri-overlay-$Arch.json")
    # PowerShell 5.1's ConvertTo-Json defaults to depth 2 — passes here
    # because the structure is shallow; bump it for safety.
    $obj | ConvertTo-Json -Depth 6 | Set-Content -Path $path -Encoding UTF8
    return $path
}

$hostArch = Get-HostArch
Write-Host ("Host architecture: {0}" -f $hostArch) -ForegroundColor DarkGray

# Build the frontend ONCE up front. The arch loop's bundle phase used
# to depend on tauri's `beforeBuildCommand` to invoke `pnpm build`, but
# every pnpm invocation rewrites frontend/dist with fresh timestamps —
# which invalidates cove-studio's cargo fingerprint between phase 1 and
# phase 2, defeating the DLL sweep. Building the frontend once before
# the loop and silencing `beforeBuildCommand` in the bundle-phase
# overlay lets cargo be a true incremental no-op on the second pass.
Write-Host ''
Write-Host '=== Frontend build (once, shared across arches) ===' -ForegroundColor Cyan
& pnpm --dir frontend build
if ($LASTEXITCODE -ne 0) { throw "pnpm build (frontend) failed (exit $LASTEXITCODE)" }

$built = @()
foreach ($arch in $archesToBuild) {
    $triple = $tripleByArch[$arch]
    Write-Host ''
    Write-Host "=== Build $arch ($triple) ===" -ForegroundColor Cyan

    # 1. Locate the Visual Studio install that carries the C++ build
    #    tools for this specific arch. Fails with a clear error if the
    #    matching workload isn't installed — much better than letting
    #    link.exe blow up later on a "machine type" mismatch.
    $vsInstall = Get-VsInstallPath -Requires @($workloadByArch[$arch])
    $vsDev = Join-Path $vsInstall 'Common7\Tools\VsDevCmd.bat'
    if (-not (Test-Path $vsDev)) {
        throw "VsDevCmd.bat not found at $vsDev (Visual Studio at $vsInstall)."
    }
    Write-Host ("VS install : {0}" -f $vsInstall) -ForegroundColor DarkGray
    Write-Host ("VsDevCmd   : {0} -arch={1} -host_arch={2}" -f $vsDev, $vsArchByArch[$arch], $hostArch) -ForegroundColor DarkGray

    $bundleRoot = Join-Path $RepoRoot "target\$triple\release\bundle"
    if ($Clean -and (Test-Path $bundleRoot)) {
        Write-Host "Cleaning $bundleRoot" -ForegroundColor DarkGray
        Remove-Item -Recurse -Force $bundleRoot
    }

    $overlay = New-ResourcesOverlay -Arch $arch
    Write-Host "Overlay    : $overlay" -ForegroundColor DarkGray

    # 2. Build → sweep → bundle, in two `tauri build` invocations
    #    sharing the same VsDevCmd-primed cmd subprocess each time
    #    (PATH/INCLUDE/LIB don't persist between separate cmd.exe
    #    calls). The sweep in between exists because
    #    `gliner2_inference v0.5.0` depends on `ort` with default
    #    features (cuda + directml + coreml + download-binaries +
    #    copy-dylibs). Cargo features are additive across the dep
    #    graph, so even though our own `ort` dep is
    #    `default-features = false, features = ["load-dynamic"]`,
    #    gliner2 still flips every EP feature on. The build script
    #    fetches the full ONNX Runtime distribution (a 617 MB CUDA
    #    EP DLL on x64) and `copy-dylibs` replicates them next to
    #    the output binary. Tauri's MSI bundler auto-includes every
    #    adjacent DLL — that's how the previous build shipped a
    #    161 MB x64 MSI carrying CUDA/TensorRT/shared EPs we never
    #    use (we load onnxruntime.dll dynamically out of
    #    libs/<arch>/, CPU EP only). Sweeping these between cargo
    #    and the bundler keeps the MSI lean without forking
    #    gliner2_inference just to disable its default features.
    #    --bundles msi overrides the conf's `bundle.targets: "all"`
    #    to skip the NSIS .exe; the base + overlay configs together
    #    produce a single MSI with the matching arch DLLs bundled
    #    into <install>/resources/libs/<lib>/win-<arch>/.
    # Phase 1 uses `cargo build` directly because the Tauri CLI we
    # ship rejects `--bundles none` (only knows `msi` / `nsis`). The
    # cove-studio binary doesn't need the frontend dist to compile —
    # frontend assets are bundled at WiX time in phase 2 — so we can
    # safely skip `pnpm build` here too. Phase 2's `tauri build` then
    # runs `pnpm build` via `beforeBuildCommand`, finds cargo already
    # up-to-date, and goes straight to the WiX bundler.
    $cmdLineBuild = '"{0}" -arch={1} -host_arch={2} -no_logo && cargo build --release --target {3} --manifest-path src-tauri\Cargo.toml' -f `
        $vsDev, $vsArchByArch[$arch], $hostArch, $triple

    & cmd.exe /c $cmdLineBuild
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build (phase 1) failed for $triple (exit $LASTEXITCODE). Check the cl.exe/link.exe output above for the failing step."
    }

    $releaseDir = Join-Path $RepoRoot "target\$triple\release"
    $strayDlls = @(
        'onnxruntime_providers_cuda.dll',
        'onnxruntime_providers_tensorrt.dll',
        'onnxruntime_providers_shared.dll',
        'onnxruntime.dll'
    )
    foreach ($name in $strayDlls) {
        $path = Join-Path $releaseDir $name
        if (Test-Path $path) {
            Remove-Item -Force $path
            Write-Host ("Swept   : {0}" -f $name) -ForegroundColor DarkGray
        }
    }

    # Signing happens here, between the two phases: WiX packages the
    # binary as it finds it, so the signature has to be on the file
    # before the bundler runs. (Tauri's own `bundle.windows.signCommand`
    # would sign at the right moment too, but it resolves the command
    # against its own working directory and PATH — inside the
    # VsDevCmd-primed subprocess that fails with a bare
    # "failed to run powershell", which tells you nothing. Signing from
    # here keeps the paths under our control.)
    if ($Sign) {
        $exe = Join-Path $releaseDir 'cove-studio.exe'
        if (-not (Test-Path $exe)) { throw "Binary to sign not found: $exe" }
        # sign-windows.ps1 throws on failure and $ErrorActionPreference
        # is Stop, so no exit-code check: $LASTEXITCODE would carry the
        # code of the last *native* command, not of the script.
        & (Join-Path $PSScriptRoot 'sign-windows.ps1') $exe
    }

    # Stale MSIs from an earlier run must go before bundling: the
    # collection step below takes every .msi it finds, so leaving one
    # behind means copying an old installer into dist/ — and, with
    # -Sign, signing it. A signed installer of a previous version
    # sitting next to the current one is a publishing accident waiting
    # to happen. (`-Clean` wipes the whole bundle dir; this runs
    # always.)
    $msiOutDir = Join-Path $bundleRoot 'msi'
    if (Test-Path $msiOutDir) {
        foreach ($old in @(Get-ChildItem -Path $msiOutDir -Filter '*.msi' -File)) {
            Remove-Item -LiteralPath $old.FullName -Force
            Write-Host ("Removed stale : {0}" -f $old.Name) -ForegroundColor DarkGray
        }
    }

    $cmdLineBundle = '"{0}" -arch={1} -host_arch={2} -no_logo && "{3}" build --config "{4}" --config "{5}" --target {6} --bundles msi' -f `
        $vsDev, $vsArchByArch[$arch], $hostArch, $tauriBin, $config, $overlay, $triple

    & cmd.exe /c $cmdLineBundle
    if ($LASTEXITCODE -ne 0) {
        throw "tauri build (bundle phase) failed for $triple (exit $LASTEXITCODE)."
    }

    $msiSrcDir = Join-Path $bundleRoot 'msi'
    if (-not (Test-Path $msiSrcDir)) {
        throw "Expected MSI output directory not found: $msiSrcDir"
    }
    $msiFiles = Get-ChildItem -Path $msiSrcDir -Filter '*.msi' -File
    if ($msiFiles.Count -eq 0) {
        throw "tauri build for $triple produced no .msi in $msiSrcDir"
    }
    # Collect produced artefacts into dist/ with a short arch suffix.
    # Tauri names MSIs `<Product>_<Version>_<arch>_<locale>.msi`; we
    # strip the locale tail and replace any pre-existing arch tail with
    # our short label so x64 and arm64 coexist in dist/ without
    # colliding. Kept as a plain `foreach` (no pipeline) and using only
    # explicit string ops so it can't trigger PS parameter-binding
    # surprises across iterations.
    foreach ($msi in @($msiFiles)) {
        $stem  = [System.IO.Path]::GetFileNameWithoutExtension($msi.Name)
        $cleanA = [regex]::Replace($stem,  '_[a-z]{2}-[A-Z]{2}$', '')
        $cleanB = [regex]::Replace($cleanA, '_(x64|arm64|x86|aarch64)$', '')
        $destName = '{0}_{1}.msi' -f $cleanB, $arch
        $destPath = Join-Path -Path $distDir -ChildPath $destName
        Copy-Item -LiteralPath $msi.FullName -Destination $destPath -Force
        # The copy in dist/ is what gets published, so that is the file
        # that must carry the signature.
        if ($Sign) {
            & (Join-Path $PSScriptRoot 'sign-windows.ps1') $destPath
        }
        Write-Host ('  -> dist\{0}' -f $destName) -ForegroundColor Green
        $built += $destPath
    }
}

Write-Host ''
Write-Host '=== Done ===' -ForegroundColor Green
Get-ChildItem -Path $distDir -Filter '*.msi' -File |
    Sort-Object Name |
    Format-Table @{n='File';e={$_.Name}}, @{n='Size (MB)';e={[math]::Round($_.Length / 1MB, 1)}}, LastWriteTime -AutoSize
