param(
    [string]$ProfilePath = ".\scripts\ollama-context-profiles.json",
    [switch]$Apply
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Get-Command ollama -ErrorAction SilentlyContinue)) {
    throw 'Ollama command not found in PATH.'
}

if (-not (Test-Path $ProfilePath)) {
    throw "Profile file not found: $ProfilePath"
}

$profileData = Get-Content -Raw -Path $ProfilePath | ConvertFrom-Json
if (-not $profileData.profiles -or $profileData.profiles.Count -eq 0) {
    throw 'No profiles found in JSON file.'
}

$tmpRoot = Join-Path $env:TEMP ("covestudio-ollama-profiles-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmpRoot -Force | Out-Null

try {
    foreach ($p in $profileData.profiles) {
        $baseModel = [string]$p.base_model
        $aliasModel = [string]$p.alias_model
        $numCtx = [int]$p.num_ctx

        if ([string]::IsNullOrWhiteSpace($baseModel) -or [string]::IsNullOrWhiteSpace($aliasModel) -or $numCtx -le 0) {
            throw "Invalid profile entry: $($p | ConvertTo-Json -Compress)"
        }

        $modelfilePath = Join-Path $tmpRoot (($aliasModel -replace '[^a-zA-Z0-9._-]', '_') + '.Modelfile')
        @(
            "FROM $baseModel"
            "PARAMETER num_ctx $numCtx"
        ) | Set-Content -Path $modelfilePath -Encoding UTF8

        if ($Apply) {
            Write-Output "[apply] ollama create $aliasModel -f $modelfilePath"
            & ollama create $aliasModel -f $modelfilePath
            if ($LASTEXITCODE -ne 0) {
                throw "Failed creating alias model: $aliasModel"
            }
        } else {
            Write-Output "[dry-run] would create $aliasModel from $baseModel with num_ctx=$numCtx"
            Write-Output "          command: ollama create $aliasModel -f $modelfilePath"
        }
    }

    if (-not $Apply) {
        Write-Output ""
        Write-Output "Dry-run completed. Re-run with -Apply to create/update aliases."
    } else {
        Write-Output ""
        Write-Output "Apply completed. Run verify script to check effective context length."
    }
}
finally {
    if (Test-Path $tmpRoot) {
        Remove-Item -Path $tmpRoot -Recurse -Force
    }
}
