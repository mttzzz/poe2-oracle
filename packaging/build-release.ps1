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

  A release signed through SignPath (Authenticode; release.yml, packaging\signpath) needs two
  signing requests, since the installer must carry the exe and the uninstaller signed already:
  -Stage Binaries builds what the first one signs, -Stage Installer packs the signed files into the
  installer that the second one signs. SHA256SUMS is taken of that signed installer afterwards.

.PARAMETER Tag
  The release tag being built (vX.Y.Z). The build stops unless it is "v" + the workspace version,
  so a release never carries an exe reporting another version than its tag.

.PARAMETER Stage
  Release, the default: all of the above.
  Binaries: the exe and the uninstaller to sign, as target\signing\poe2-oracle.exe and
  target\signing\uninstall.exe (packaging\installer.nsi's EXPORT_UNINST), and
  target\dist\THIRD-PARTY-NOTICES.html. No installer, no SHA256SUMS.
  Installer: target\dist\PoE2-Oracle-Setup-<version>.exe carrying the signed exe and uninstaller
  from -SignedBinaries (IMPORT_UNINST) and the THIRD-PARTY-NOTICES.html target\dist must hold
  already. Nothing is compiled: the version is the one the signed exe reports. No SHA256SUMS.

.PARAMETER SignedBinaries
  With -Stage Installer only: the folder holding poe2-oracle.exe and uninstall.exe, both with a
  valid Authenticode signature.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File packaging\build-release.ps1

.EXAMPLE
  $env:RELEASE_SIGNING_KEY = Get-Content D:\keys\release-signing-key.txt
  powershell -NoProfile -ExecutionPolicy Bypass -File packaging\build-release.ps1 -Tag v0.1.0
#>
[CmdletBinding()]
param(
    [string]$Tag,
    [ValidateSet('Release', 'Binaries', 'Installer')]
    [string]$Stage = 'Release',
    [string]$SignedBinaries
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Windows PowerShell 5.1 redraws its progress bar per received chunk, slowing downloads many-fold.
$ProgressPreference = 'SilentlyContinue'

if (($Stage -eq 'Installer') -ne [bool]$SignedBinaries) {
    throw '-SignedBinaries goes with -Stage Installer, which needs it: the folder of the signed poe2-oracle.exe and uninstall.exe'
}
# Relative to where the caller is, not to the repository root this script works in.
if ($SignedBinaries) { $SignedBinaries = (Resolve-Path -LiteralPath $SignedBinaries).Path }

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

# A signed installer must carry a signed exe and uninstaller: after the install, Windows (Smart App
# Control, SmartScreen) and antivirus judge the installed files, not the installer.
function Assert-Signed {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "$Path is missing" }
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid') {
        throw "$Path has no valid Authenticode signature ($($signature.Status)): $($signature.StatusMessage)"
    }
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
    $dist = Join-Path $root 'target\dist'
    if ($Stage -eq 'Installer') {
        # The Binaries stage built these files and checked the service settings and the tag; here,
        # their signatures, and the version is the one the exe reports.
        $exe = Join-Path $SignedBinaries 'poe2-oracle.exe'
        $uninstaller = Join-Path $SignedBinaries 'uninstall.exe'
        Assert-Signed $exe
        Assert-Signed $uninstaller
        $version = (Get-Item -LiteralPath $exe).VersionInfo.ProductVersion
        if ($Tag -and $Tag -ne "v$version") {
            throw "Tag $Tag does not match the version $version of $exe"
        }
        $notices = Join-Path $dist 'THIRD-PARTY-NOTICES.html'
        if (-not (Test-Path -LiteralPath $notices)) {
            throw "$notices is missing: the installer carries the one -Stage Binaries wrote"
        }
        # An installer or SHA256SUMS left from another build would describe other files.
        Get-ChildItem -LiteralPath $dist | Where-Object { $_.Name -ne 'THIRD-PARTY-NOTICES.html' } |
            Remove-Item -Recurse -Force
        $makensis = Get-MakeNsis
        Invoke-Native $makensis @('/INPUTCHARSET', 'UTF8', "/DVERSION=$version", "/DAPP_EXE_PATH=$exe",
            "/DIMPORT_UNINST=$uninstaller", "/DOUT_DIR=$dist", (Join-Path $PSScriptRoot 'installer.nsi'))
        Get-ChildItem $dist | Format-Table Name, Length -AutoSize
        return
    }

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
    if ($Stage -eq 'Release' -and -not $signingKey) { Write-Warning $unsigned }

    # Before the long build, so a cargo-about that can't be installed stops the release early.
    Install-CargoAbout

    Invoke-Native cargo @('build', '-p', 'poe2-oracle', '--release', '--locked')
    $exe = Join-Path $root 'target\release\poe2-oracle.exe'

    $makensis = Get-MakeNsis
    if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
    New-Item -ItemType Directory -Path $dist | Out-Null
    # Into the installer, next to the exe: the data, the fonts and every crate of the shipped
    # app's own graph (-m).
    Invoke-Native cargo @('about', 'generate', '--locked',
        '-m', (Join-Path $root 'crates\poe2-oracle\Cargo.toml'),
        '-c', (Join-Path $root 'about.toml'),
        '-o', (Join-Path $dist 'THIRD-PARTY-NOTICES.html'),
        (Join-Path $root 'about.hbs'))
    if ($Stage -eq 'Binaries') {
        # What the first signing request signs: the exe, and the uninstaller makensis generates for
        # this installer.
        $binaries = Join-Path $root 'target\signing'
        if (Test-Path $binaries) { Remove-Item -Recurse -Force $binaries }
        New-Item -ItemType Directory -Path $binaries | Out-Null
        Copy-Item -LiteralPath $exe -Destination $binaries
        $uninstaller = Join-Path $binaries 'uninstall.exe'
        Invoke-Native $makensis @('/INPUTCHARSET', 'UTF8', "/DVERSION=$version", "/DAPP_EXE_PATH=$exe",
            "/DEXPORT_UNINST=$uninstaller", "/DOUT_DIR=$dist", (Join-Path $PSScriptRoot 'installer.nsi'))
        if (-not (Test-Path -LiteralPath $uninstaller)) {
            throw "makensis exported no uninstaller to $uninstaller"
        }
        # This pass's installer carries the unsigned uninstaller: -Stage Installer makes the real one.
        Remove-Item -LiteralPath (Join-Path $dist "PoE2-Oracle-Setup-$version.exe")
        Get-ChildItem $binaries, $dist | Format-Table Name, Length -AutoSize
        return
    }
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
