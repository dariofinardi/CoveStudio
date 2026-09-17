# Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
#requires -Version 5.1
<#
.SYNOPSIS
  Sign a Windows binary or installer with Azure Trusted Signing.

.DESCRIPTION
  Wraps `signtool` with the Trusted Signing dlib, which asks the Azure
  service for a short-lived certificate instead of holding a private key
  on disk. Credentials come from the Azure CLI session (`az login`) via
  DefaultAzureCredential, so nothing secret lives in the repo.

  Tauri calls this script once per artefact through
  `bundle.windows.signCommand`, passing the file to sign; it also runs
  standalone to sign an MSI after the fact.

  Requirements, checked below with a clear message each:
   - `signtool.exe` from the Windows SDK;
   - the Trusted Signing client (NuGet `Microsoft.Trusted.Signing.Client`)
     expanded somewhere — by default `%LOCALAPPDATA%\TrustedSigningClient`;
   - an `az login` session whose identity holds the *Trusted Signing
     Certificate Profile Signer* role on the account below.

  The dlib ships for x64/x86 only, so on an ARM64 host we deliberately
  pick the **x64** signtool and let the emulation layer run it.

.PARAMETER Path
  File to sign (.exe, .dll, .msi). Accepts several.

.PARAMETER Endpoint
  Trusted Signing account endpoint. Default: the Jugaad account.

.PARAMETER Account
  Code-signing account name.

.PARAMETER Profile
  Certificate profile name.

.PARAMETER ClientRoot
  Where the Trusted Signing client was expanded.

.EXAMPLE
  ./scripts/sign-windows.ps1 dist/CoveStudio_0.8.0_arm64_en-US.msi
  ./scripts/sign-windows.ps1 (Get-ChildItem dist/*.msi).FullName
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0, ValueFromRemainingArguments = $true)]
    [string[]]$Path,

    [string]$Endpoint = 'https://neu.codesigning.azure.net/',
    [string]$Account = 'Jugaad-srl',
    [string]$Profile = 'Jugaad-srl',
    [string]$ClientRoot = "$env:LOCALAPPDATA\TrustedSigningClient",
    # Microsoft's public timestamp authority for Trusted Signing.
    [string]$TimestampUrl = 'http://timestamp.acs.microsoft.com'
)

$ErrorActionPreference = 'Stop'

$dlib = Join-Path $ClientRoot 'bin\x64\Azure.CodeSigning.Dlib.dll'
if (-not (Test-Path $dlib)) {
    throw @"
Trusted Signing client not found at $dlib.
Install it with:
  Invoke-WebRequest https://www.nuget.org/api/v2/package/Microsoft.Trusted.Signing.Client -OutFile `$env:TEMP\tsc.zip
  Expand-Archive `$env:TEMP\tsc.zip -DestinationPath `$env:LOCALAPPDATA\TrustedSigningClient -Force
"@
}

# Newest SDK first; x64 because the dlib has no ARM64 build.
$signtool = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\x64\\' } |
    Sort-Object FullName -Descending |
    Select-Object -First 1 -ExpandProperty FullName
if (-not $signtool) {
    throw 'signtool.exe (x64) not found. Install the Windows SDK "Signing Tools" component.'
}

# The dlib reads the account coordinates from a JSON file, not argv.
#
# ExcludeCredentials is not optional: the dlib authenticates with
# DefaultAzureCredential, which tries Visual Studio, VS Code, Azure
# PowerShell and others *before* the Azure CLI. If any of those is
# signed in with a different account, the service gets a perfectly valid
# token for an identity without the signer role and answers 403 — a
# failure that looks exactly like a missing role assignment. Leaving
# only AzureCliCredential ties signing to `az login` and nothing else.
$metadata = Join-Path $env:TEMP 'cove-studio-trusted-signing.json'
[ordered]@{
    Endpoint                 = $Endpoint
    CodeSigningAccountName   = $Account
    CertificateProfileName   = $Profile
    ExcludeCredentials       = @(
        'EnvironmentCredential', 'WorkloadIdentityCredential',
        'ManagedIdentityCredential', 'SharedTokenCacheCredential',
        'VisualStudioCredential', 'VisualStudioCodeCredential',
        'AzurePowerShellCredential', 'AzureDeveloperCliCredential',
        'InteractiveBrowserCredential'
    )
} | ConvertTo-Json | Set-Content -Path $metadata -Encoding UTF8

$failed = @()
foreach ($item in $Path) {
    $file = (Resolve-Path -LiteralPath $item).Path
    Write-Host "Signing $file" -ForegroundColor Cyan
    # The first attempt after a period of inactivity fails regularly:
    # the Azure CLI token is renewed inside the dlib and the call in
    # flight is lost. Three attempts with a growing pause, then a real
    # error.
    $signed = $false
    foreach ($attempt in 1..3) {
        & $signtool sign /v /fd SHA256 /tr $TimestampUrl /td SHA256 `
            /dlib $dlib /dmdf $metadata $file
        if ($LASTEXITCODE -eq 0) { $signed = $true; break }
        Write-Warning "Signing failed (attempt $attempt of 3): $file"
        if ($attempt -lt 3) { Start-Sleep -Seconds (10 * $attempt) }
    }
    if (-not $signed) {
        $failed += $file
        continue
    }
    # A signature the OS cannot chain to a trusted root is worse than no
    # signature: it tells the user the file was tampered with.
    & $signtool verify /pa /v $file | Out-Null
    if ($LASTEXITCODE -ne 0) { $failed += $file }
}

if ($failed.Count -gt 0) {
    throw "Signing failed for: $($failed -join ', ')"
}
Write-Host "Signed $($Path.Count) file(s)." -ForegroundColor Green
