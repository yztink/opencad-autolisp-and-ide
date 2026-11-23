<#
.SYNOPSIS
    Synchronizes acadrust revision between OpenCADStudio and opencad-lisp-and-ide,
    and optionally builds and installs the plugin into OpenCAD Studio.

.DESCRIPTION
    OpenCAD Studio verifies that installed plugins match the exact git commit hash of acadrust
    it was compiled with. When acadrust is bumped in OpenCAD Studio, this script:
      1. Reads the acadrust revision and full commit hash from OpenCAD Studio's Cargo.lock.
      2. Updates plugin.toml's acadrust_source with the full commit hash.
      3. Updates Cargo.lock using `cargo update -p acadrust`.
      4. Optionally compiles `--release` (-Build) and installs to %APPDATA%\OpenCADStudio\plugins\opencad.lisp_and_ide\ (-Install).

.PARAMETER OcsPath
    Path to OpenCADStudio repository. Defaults to checking ../OpenCADStudio-fork, ../OpenCADStudio, or ../OpenCADStudio-2026.37.

.PARAMETER Build
    If specified, runs `cargo build --release` after syncing.

.PARAMETER Install
    If specified, builds release and copies plugin.toml and opencad.lisp_and_ide.dll to %APPDATA%\OpenCADStudio\plugins\opencad.lisp_and_ide\.

.EXAMPLE
    .\scripts\sync-acadrust.ps1
    Syncs dependency configuration without building.

.EXAMPLE
    .\scripts\sync-acadrust.ps1 -Install
    Syncs dependency configuration, compiles release, and deploys directly to OpenCAD Studio plugin directory.
#>
[CmdletBinding()]
param(
    [string]$OcsPath,
    [switch]$Build,
    [switch]$Install
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RunnerRoot = Split-Path -Parent $ScriptDir

# 1. Locate OpenCAD Studio
if (-not $OcsPath) {
    $candidates = @(
        (Join-Path (Split-Path -Parent $RunnerRoot) "OpenCADStudio-upstream-main"),
        (Join-Path (Split-Path -Parent $RunnerRoot) "OpenCADStudio-pr-selection"),
        (Join-Path (Split-Path -Parent $RunnerRoot) "OpenCADStudio-fork"),
        (Join-Path (Split-Path -Parent $RunnerRoot) "OpenCADStudio"),
        (Join-Path (Split-Path -Parent $RunnerRoot) "OpenCADStudio-2026.37")
    )
    foreach ($cand in $candidates) {
        if (Test-Path (Join-Path $cand "Cargo.lock")) {
            $OcsPath = $cand
            break
        }
    }
}

if (-not $OcsPath -or -not (Test-Path (Join-Path $OcsPath "Cargo.lock"))) {
    Write-Error "Could not find OpenCADStudio repository with Cargo.lock. Specify with -OcsPath <path>."
    exit 1
}

Write-Host "Using OpenCADStudio at: $OcsPath" -ForegroundColor Cyan

# 2. Extract codec source from OpenCADStudio's Cargo.lock
$ocsLock = Get-Content (Join-Path $OcsPath "Cargo.lock") -Raw
$codecPkgName = "opencadcodec"
if ($ocsLock -match '\[\[package\]\]\r?\nname = "opencadcodec"\r?\nversion = "[^"]+"\r?\nsource = "([^"]+)"') {
    $codecPkgName = "opencadcodec"
    $acadrustSource = $Matches[1]
} elseif ($ocsLock -match '\[\[package\]\]\r?\nname = "acadrust"\r?\nversion = "[^"]+"\r?\nsource = "([^"]+)"') {
    $codecPkgName = "acadrust"
    $acadrustSource = $Matches[1]
} else {
    Write-Error "Could not find opencadcodec or acadrust package in $OcsPath\Cargo.lock"
    exit 1
}

Write-Host "Found OpenCADStudio $codecPkgName source: $acadrustSource" -ForegroundColor Green

# Extract rev query param if present
$rev = ""
if ($acadrustSource -match 'rev=([^#&]+)') {
    $rev = $Matches[1]
} else {
    Write-Error "Could not parse rev from codec source: $acadrustSource"
    exit 1
}

# 3. Update plugin.toml
$pluginTomlPath = Join-Path $RunnerRoot "plugin.toml"
if (Test-Path $pluginTomlPath) {
    $content = Get-Content $pluginTomlPath -Raw
    $newContent = [regex]::Replace(
        $content,
        'acadrust_source = "[^"]+"',
        "acadrust_source = `"$acadrustSource`""
    )
    if ($newContent -ne $content) {
        Set-Content -Path $pluginTomlPath -Value $newContent -NoNewline
        Write-Host "Updated $pluginTomlPath to acadrust_source = `"$acadrustSource`"" -ForegroundColor Green
    } else {
        Write-Host "$pluginTomlPath already has matching acadrust_source" -ForegroundColor DarkGray
    }
}

# 4. Run cargo update
Write-Host "Updating Cargo.lock in opencad-lisp-and-ide..." -ForegroundColor Cyan
Push-Location $RunnerRoot
try {
    & cargo update -p $codecPkgName
    if ($LASTEXITCODE -ne 0) {
        Write-Error "cargo update -p $codecPkgName failed with code $LASTEXITCODE"
        exit $LASTEXITCODE
    }

    if ($Build -or $Install) {
        Write-Host "Building release binaries..." -ForegroundColor Cyan
        & cargo build --release
        if ($LASTEXITCODE -ne 0) {
            Write-Error "cargo build --release failed with code $LASTEXITCODE"
            exit $LASTEXITCODE
        }
    }

    if ($Install) {
        $appDataPluginDir = Join-Path $env:APPDATA "OpenCADStudio\plugins\opencad.lisp_and_ide"
        if (-not (Test-Path $appDataPluginDir)) {
            New-Item -ItemType Directory -Path $appDataPluginDir -Force | Out-Null
        }
        Copy-Item -Force (Join-Path $RunnerRoot "plugin.toml") (Join-Path $appDataPluginDir "plugin.toml")
        Copy-Item -Force (Join-Path $RunnerRoot "target\release\opencad_lisp.dll") (Join-Path $appDataPluginDir "opencad.lisp_and_ide.dll")
        if (Test-Path (Join-Path $RunnerRoot "target\release\opencad_lisp_ide.exe")) {
            Copy-Item -Force (Join-Path $RunnerRoot "target\release\opencad_lisp_ide.exe") (Join-Path $appDataPluginDir "opencad_lisp_ide.exe")
        }
        Write-Host "Successfully installed plugin to $appDataPluginDir" -ForegroundColor Green
    }
} finally {
    Pop-Location
}

Write-Host "Synchronization complete!" -ForegroundColor Green
