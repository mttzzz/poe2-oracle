#Requires -Version 5.1
<#
.SYNOPSIS
  Builds a PoE2 Oracle release on Windows: target\release\poe2-oracle.exe, then
  target\dist\PoE2-Oracle-Setup-<version>.exe, target\dist\SHA256SUMS, target\dist\SHA256SUMS.sig
  when a signing key is given, and target\dist\THIRD-PARTY-NOTICES.html.

.DESCRIPTION
  Needs Rust with the MSVC toolchain and the Windows SDK: gpui compiles its shaders with the SDK's
  fxc.exe in release builds, and crates\poe2-oracle\build.rs embeds the exe's icon and version
  with its rc.exe. makensis comes from PATH or a standard NSIS install; failing both, the official
  NSIS 3.12 zip is downloaded into %TEMP% and checked against its pinned SHA-256. cargo-about
  writes the third-party notices from about.toml and about.hbs; a missing one, or another version
  than the pinned one, is installed with `cargo install` first. The installer carries the notices
  and the two license texts next to the exe.
  The build stops if a test build's service settings are on: POE2_ORACLE_API_BASE in the
  environment, or oracle-protocol's dev-endpoints feature (see API_BASE in
  crates\oracle-protocol\src\lib.rs). A release talks to https://oracle.pushka.biz only.
  Installed apps take an update only when SHA256SUMS.sig is the release key's signature of
  SHA256SUMS. With RELEASE_SIGNING_KEY set to the key's seed (the one line `release-sign keygen`
  wrote), the script signs SHA256SUMS with crates\release-sign and checks the signature against
  the public key the app carries, crates\auto-update\release-signing-key.pub. The seed stays out
  of the environment of everything else the build runs. Without it the release is unsigned, which
  the script warns about: installed apps refuse to update to it until it is signed.
  .github\workflows\release.yml runs this same script, unsigned, and signs in a job of its own.

.PARAMETER Tag
  The release tag being built (vX.Y.Z). The build stops unless it is "v" + the workspace version,
  so a release never carries an exe reporting another version than its tag.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File packaging\build-release.ps1

.EXAMPLE
  $env:RELEASE_SIGNING_KEY = Get-Content D:\keys\release-signing-key.txt
  powershell -NoProfile -ExecutionPolicy Bypass -File packaging\build-release.ps1 -Tag v0.1.0
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
# Every build script and proc-macro of the app's dependency graph runs in this environment, so the
# release key's seed leaves it at once; release-sign alone gets it back, and the caller's session
# has it again at the end.
$signingKey = $env:RELEASE_SIGNING_KEY
Remove-Item Env:RELEASE_SIGNING_KEY -ErrorAction SilentlyContinue
$unsigned = 'RELEASE_SIGNING_KEY is not set, so this release is UNSIGNED: target\dist gets no ' +
    'SHA256SUMS.sig, and installed apps refuse to update to it. Set it and build again, or sign ' +
    'SHA256SUMS alone: cargo run -p release-sign -- sign target\dist\SHA256SUMS --key-file <seed file>'
Push-Location $root
try {
    # Read at build time into the app (option_env!): a test service's address left in this shell
    # would ship in the exe.
    if (Test-Path Env:POE2_ORACLE_API_BASE) {
        throw "POE2_ORACLE_API_BASE is set to '$env:POE2_ORACLE_API_BASE': a release talks to https://oracle.pushka.biz. Remove it (Remove-Item Env:POE2_ORACLE_API_BASE) and build again."
    }
    $metadata = (Invoke-Native cargo @('metadata', '--format-version', '1', '--no-deps', '--locked') |
        Out-String) | ConvertFrom-Json
    $version = ($metadata.packages | Where-Object { $_.name -eq 'poe2-oracle' }).version
    if ($Tag -and $Tag -ne "v$version") {
        throw "Tag $Tag does not match the workspace version $version (root Cargo.toml)"
    }
    # The features the release build gives oracle-protocol, as cargo resolves them: `{f}` lists
    # them after the package. dev-endpoints, on only in a test build, lets the app talk to a
    # plain-http service.
    $protocol = Invoke-Native cargo @('tree', '--locked', '-p', 'poe2-oracle', '-i', 'oracle-protocol',
        '-e', 'normal', '--prefix', 'none', '--format', '{p} {f}') |
        Where-Object { $_ -like 'oracle-protocol *' }
    if (-not $protocol) {
        throw "cargo tree did not list oracle-protocol, so its features are unknown"
    }
    if ($protocol -match 'dev-endpoints') {
        throw "oracle-protocol is built with its test-only dev-endpoints feature: $protocol"
    }
    # Said now too, while the long build can still be stopped to set the key.
    if (-not $signingKey) { Write-Warning $unsigned }

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
    $sums = Join-Path $dist 'SHA256SUMS'
    # sha256sum's text format with an LF line ending: what crates\auto-update parses.
    [IO.File]::WriteAllText($sums, "$hash  $(Split-Path -Leaf $installer)`n")
    if ($signingKey) {
        Invoke-Native cargo @('build', '-p', 'release-sign', '--release', '--locked')
        $releaseSign = Join-Path $root 'target\release\release-sign.exe'
        $env:RELEASE_SIGNING_KEY = $signingKey
        try {
            Invoke-Native $releaseSign @('sign', $sums, "$sums.sig")
        } finally {
            Remove-Item Env:RELEASE_SIGNING_KEY
        }
        # Signed with another key than the one the app carries, the release would be refused by
        # every installed copy.
        $publicKey = (Get-Content -Raw (Join-Path $root 'crates\auto-update\release-signing-key.pub')).Trim()
        try {
            Invoke-Native $releaseSign @('verify', $sums, "$sums.sig", $publicKey)
        } catch {
            Remove-Item "$sums.sig"
            throw "RELEASE_SIGNING_KEY is not the release key: its public half isn't crates\auto-update\release-signing-key.pub, so installed apps would refuse this release."
        }
    }
    Get-ChildItem $dist | Format-Table Name, Length -AutoSize
    if (-not $signingKey) { Write-Warning $unsigned }
} finally {
    Pop-Location
    if ($signingKey) { $env:RELEASE_SIGNING_KEY = $signingKey }
}
