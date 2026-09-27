#Requires -Version 5.1
<#
.SYNOPSIS
  Writes the WinGet manifests of a published PoE2 Oracle release, ready for a pull request to
  microsoft/winget-pkgs: <OutDir>\manifests\m\mttzzz\PoE2Oracle\<version>\*.yaml.

.DESCRIPTION
  Fills the templates next to this script (mttzzz.PoE2Oracle*.yaml) with the version, the
  installer's URL, its SHA-256 and the release date. The SHA-256 is the one of the file the URL
  serves, downloaded here, which must also match the installer's line in the release's SHA256SUMS
  next to it: a manifest never names a file the release pipeline didn't make. The release date is
  the GitHub release's, or -ReleaseDate, and left out when neither is to be had.

  The installer URL is the GitHub release asset by default: it stays up for every version, where
  https://oracle.pushka.biz/download/v<version>/ serves the latest release only, and WinGet keeps
  the manifests of older versions.

.PARAMETER Version
  The release's version, X.Y.Z (a leading "v" is dropped).

.PARAMETER InstallerUrl
  Where WinGet downloads the installer. Default:
  https://github.com/mttzzz/poe2-oracle/releases/download/v<version>/PoE2-Oracle-Setup-<version>.exe

.PARAMETER ReleaseDate
  The release date, yyyy-MM-dd. Default: the GitHub release's publication date.

.PARAMETER OutDir
  Where the manifests tree goes. Default: target\winget in the repository.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File packaging\winget\update-manifests.ps1 -Version 0.1.0
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string]$Version,
    [string]$InstallerUrl,
    [string]$ReleaseDate,
    [string]$OutDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Windows PowerShell 5.1 redraws its progress bar per received chunk, slowing downloads many-fold.
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol =
    [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$identifier = 'mttzzz.PoE2Oracle'
$repository = 'mttzzz/poe2-oracle'
$Version = $Version -replace '^v', ''
if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') {
    throw "$Version is no version: X.Y.Z or X.Y.Z-pre"
}
$installerName = "PoE2-Oracle-Setup-$Version.exe"
if (-not $InstallerUrl) {
    $InstallerUrl = "https://github.com/$repository/releases/download/v$Version/$installerName"
}
if (-not $OutDir) { $OutDir = Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'target\winget' }

$temp = Join-Path ([IO.Path]::GetTempPath()) "poe2-oracle-winget-$Version"
New-Item -ItemType Directory -Force -Path $temp | Out-Null
try {
    $installer = Join-Path $temp $installerName
    Invoke-WebRequest -UseBasicParsing -Uri $InstallerUrl -OutFile $installer
    $sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToUpperInvariant()

    # SHA256SUMS sits next to the installer, on GitHub and on oracle.pushka.biz alike.
    $sumsUrl = $InstallerUrl.Substring(0, $InstallerUrl.LastIndexOf('/') + 1) + 'SHA256SUMS'
    $sums = (Invoke-WebRequest -UseBasicParsing -Uri $sumsUrl).Content
    if ($sums -is [byte[]]) { $sums = [Text.Encoding]::UTF8.GetString($sums) }
    $pattern = "^([0-9a-fA-F]{64})  $([regex]::Escape($installerName))\s*$"
    $listed = $null
    foreach ($entry in $sums -split "`n") {
        if ($entry -match $pattern) { $listed = $Matches[1].ToUpperInvariant() }
    }
    if (-not $listed) { throw "$sumsUrl has no line for $installerName" }
    if ($listed -ne $sha256) {
        throw "$InstallerUrl has SHA-256 $sha256, but $sumsUrl says $($listed): not the release's installer"
    }
} finally {
    Remove-Item -Recurse -Force $temp -ErrorAction SilentlyContinue
}

if (-not $ReleaseDate) {
    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repository/releases/tags/v$Version"
        $ReleaseDate = ([DateTime]$release.published_at).ToUniversalTime().ToString('yyyy-MM-dd')
    } catch {
        Write-Warning "No release date from GitHub ($($_.Exception.Message)): the manifest goes without one. Pass -ReleaseDate to set it."
    }
}

$utf8 = New-Object Text.UTF8Encoding $false
$target = Join-Path $OutDir "manifests\m\mttzzz\PoE2Oracle\$Version"
New-Item -ItemType Directory -Force -Path $target | Out-Null
foreach ($template in Get-ChildItem -LiteralPath $PSScriptRoot -Filter "$identifier*.yaml") {
    $text = [IO.File]::ReadAllText($template.FullName, $utf8) -replace "`r`n", "`n"
    if (-not $ReleaseDate) { $text = $text -replace '(?m)^ReleaseDate: \$\{RELEASE_DATE\}\n', '' }
    $text = $text.Replace('${VERSION}', $Version).Replace('${INSTALLER_URL}', $InstallerUrl).
        Replace('${INSTALLER_SHA256}', $sha256).Replace('${RELEASE_DATE}', "$ReleaseDate")
    if ($text -match '\$\{[A-Z_0-9]+\}') { throw "$($template.Name) still holds $($Matches[0])" }
    [IO.File]::WriteAllText((Join-Path $target $template.Name), $text, $utf8)
}

Get-ChildItem -LiteralPath $target | Format-Table Name, Length -AutoSize
Write-Host "Check, then test the install (Windows, the second one as administrator):"
Write-Host "  winget validate --manifest `"$target`""
Write-Host "  winget settings --enable LocalManifestFiles"
Write-Host "  winget install --manifest `"$target`""
