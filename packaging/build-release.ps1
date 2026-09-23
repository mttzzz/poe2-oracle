#Requires -Version 5.1
<#
.SYNOPSIS
  Builds a PoE2 Oracle release on Windows: target\release\poe2-oracle.exe, then
  target\dist\PoE2-Oracle-Setup-<version>.exe, target\dist\SHA256SUMS and
  target\dist\THIRD-PARTY-NOTICES.html.

.DESCRIPTION
  Needs Rust with the MSVC toolchain and the Windows SDK: gpui compiles its shaders with the SDK's
  fxc.exe in release builds, and crates\poe2-oracle\build.rs embeds the exe's icon and version
  with its rc.exe. makensis comes from PATH or a standard NSIS install; failing both, the official
  NSIS 3.12 zip is downloaded into %TEMP% and checked against its pinned SHA-256. cargo-about
  writes the third-party notices from about.toml and about.hbs; a missing one, or another version
  than the pinned one, is installed with `cargo install` first. The installer carries the notices
  and the two license texts next to the exe.
  The build stops if a test build's update settings are on: POE2_ORACLE_RELEASES_URL in the
  environment, or crates\auto-update's local-release-server feature (see LATEST_RELEASE_URL in
  crates\auto-update\src\lib.rs). A release's updater asks this repository's GitHub releases only.
  .github\workflows\release.yml runs this same script.

.PARAMETER Tag
  The release tag being built (vX.Y.Z). The build stops unless it is "v" + the workspace version,
  so a release never carries an exe reporting another version than its tag.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File packaging\build-release.ps1
#>
[CmdletBinding()]
param([string]$Tag)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Windows PowerShell 5.1 redraws its progress bar per received chunk, slowing downloads many-fold.
$ProgressPreference = 'SilentlyContinue'

$NsisVersion = '3.12'
# nsis-3.12.zip as SourceForge serves it; its MD5 matched the one SourceForge publishes (2026-09-22).
$NsisZipSha256 = '56581f90db321581c5381193d796fffcf2d24b2f8fed2160a6c6a3baa67f2c4f'
# The cargo-about the notices were generated and checked with (2026-09-23); another version may
# read about.toml or lay the notices out differently. Its binary is behind the `cli` feature.
$CargoAboutVersion = '0.9.2'

# Runs a native tool and fails on a non-zero exit code. Windows PowerShell 5.1 turns a native
# tool's stderr into error records once output is redirected (as over SSH or in CI), which
# 'Stop' would make fatal -- and cargo reports progress on stderr -- so it is relaxed in here.
function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments)
    $ErrorActionPreference = 'Continue'
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Exe $($Arguments -join ' ') exited with code $LASTEXITCODE" }
}

function Get-MakeNsis {
    $onPath = Get-Command makensis.exe -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    foreach ($programFiles in @(${env:ProgramFiles(x86)}, $env:ProgramFiles)) {
        if ($programFiles) {
            $installed = Join-Path $programFiles 'NSIS\makensis.exe'
            if (Test-Path $installed) { return $installed }
        }
    }

    $nsisDir = Join-Path $env:TEMP "nsis-$NsisVersion"
    $makensis = Join-Path $nsisDir "nsis-$NsisVersion\makensis.exe"
    if (Test-Path $makensis) { return $makensis }
    $zip = Join-Path $env:TEMP "nsis-$NsisVersion.zip"
    [Net.ServicePointManager]::SecurityProtocol =
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    # SourceForge answers browser-like user agents -- PowerShell's contains "Mozilla" -- with an
    # HTML download page; a wget-like one gets the file itself (checked 2026-09-22).
    Invoke-WebRequest -UseBasicParsing -UserAgent 'Wget' -OutFile $zip `
        -Uri "https://downloads.sourceforge.net/project/nsis/NSIS%203/$NsisVersion/nsis-$NsisVersion.zip"
    $actual = (Get-FileHash -Algorithm SHA256 $zip).Hash
    if ($actual -ne $NsisZipSha256) {
        Remove-Item $zip
        throw "nsis-$NsisVersion.zip has SHA-256 $actual, expected $NsisZipSha256"
    }
    Expand-Archive -Path $zip -DestinationPath $nsisDir -Force
    Remove-Item $zip
    return $makensis
}

# Makes `cargo about` the pinned version: installed when missing or another one.
function Install-CargoAbout {
    if (Get-Command cargo-about.exe -ErrorAction SilentlyContinue) {
        $installed = (Invoke-Native cargo @('about', '--version') | Out-String).Trim()
        if ($installed -eq "cargo-about $CargoAboutVersion") { return }
    }
    Invoke-Native cargo @('install', 'cargo-about', '--version', $CargoAboutVersion, '--locked',
        '--features', 'cli')
}

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    # Read at build time into the updater (option_env!): a test release server's address left in
    # this shell would ship in the exe.
    if (Test-Path Env:POE2_ORACLE_RELEASES_URL) {
        throw "POE2_ORACLE_RELEASES_URL is set to '$env:POE2_ORACLE_RELEASES_URL': a release asks GitHub. Remove it (Remove-Item Env:POE2_ORACLE_RELEASES_URL) and build again."
    }
    $metadata = (Invoke-Native cargo @('metadata', '--format-version', '1', '--no-deps', '--locked') |
        Out-String) | ConvertFrom-Json
    $version = ($metadata.packages | Where-Object { $_.name -eq 'poe2-oracle' }).version
    if ($Tag -and $Tag -ne "v$version") {
        throw "Tag $Tag does not match the workspace version $version (root Cargo.toml)"
    }
    # The features the release build gives auto-update, as cargo resolves them: `{f}` lists them
    # after the package. local-release-server, on only in a test build, lets the updater take a
    # release over plain http.
    $updater = Invoke-Native cargo @('tree', '--locked', '-p', 'poe2-oracle', '-i', 'auto-update',
        '-e', 'normal', '--prefix', 'none', '--format', '{p} {f}') |
        Where-Object { $_ -like 'auto-update *' }
    if (-not $updater) {
        throw "cargo tree did not list auto-update, so its features are unknown"
    }
    if ($updater -match 'local-release-server') {
        throw "auto-update is built with its test-only local-release-server feature: $updater"
    }

    # Before the long build, so a cargo-about that can't be installed stops the release early.
    Install-CargoAbout

    Invoke-Native cargo @('build', '-p', 'poe2-oracle', '--release', '--locked')
    $exe = Join-Path $root 'target\release\poe2-oracle.exe'

    $makensis = Get-MakeNsis
    $dist = Join-Path $root 'target\dist'
    if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
    New-Item -ItemType Directory -Path $dist | Out-Null
    # Into the installer, next to the exe: the data, the fonts and every crate of the shipped
    # app's own graph (-m).
    Invoke-Native cargo @('about', 'generate', '--locked',
        '-m', (Join-Path $root 'crates\poe2-oracle\Cargo.toml'),
        '-c', (Join-Path $root 'about.toml'),
        '-o', (Join-Path $dist 'THIRD-PARTY-NOTICES.html'),
        (Join-Path $root 'about.hbs'))
    Invoke-Native $makensis @('/INPUTCHARSET', 'UTF8', "/DVERSION=$version", "/DAPP_EXE_PATH=$exe",
        "/DOUT_DIR=$dist", (Join-Path $PSScriptRoot 'installer.nsi'))

    $installer = Join-Path $dist "PoE2-Oracle-Setup-$version.exe"
    $hash = (Get-FileHash -Algorithm SHA256 $installer).Hash.ToLowerInvariant()
    # sha256sum's text format with an LF line ending: what crates\auto-update parses.
    [IO.File]::WriteAllText((Join-Path $dist 'SHA256SUMS'), "$hash  $(Split-Path -Leaf $installer)`n")
    Get-ChildItem $dist | Format-Table Name, Length -AutoSize
} finally {
    Pop-Location
}
