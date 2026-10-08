# Installs Lume on Windows from the latest GitHub release.
#
#   irm https://raw.githubusercontent.com/Lume-agents/Lume/main/scripts/install.ps1 | iex
#
# Options are read from environment variables, so they work with `| iex`:
#   $env:LUME_VERSION = "0.15.4"   # install that release instead of the latest one
#   $env:LUME_DRY_RUN = "1"        # print what would happen without downloading
#   $env:LUME_INTERACTIVE = "1"    # run the installer window instead of a silent install
#
# The script only downloads the installer published on
# https://github.com/Lume-agents/Lume/releases and verifies nothing else on your machine.
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

# `throw` instead of `exit`, so a failed install never closes the terminal that ran `| iex`.
function Fail($message) { throw "lume-install: $message" }

if ($PSVersionTable.PSVersion.Major -lt 5) { Fail "PowerShell 5 or newer is required." }
if (-not [Environment]::Is64BitOperatingSystem) { Fail "Lume publishes a 64-bit Windows installer only." }
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$repo = "Lume-agents/Lume"
$version = ($env:LUME_VERSION -replace "^v", "")
$api = if ($version) { "https://api.github.com/repos/$repo/releases/tags/v$version" } else { "https://api.github.com/repos/$repo/releases/latest" }

try {
  $release = Invoke-RestMethod -Uri $api -Headers @{ "User-Agent" = "lume-installer"; "Accept" = "application/vnd.github+json" }
} catch {
  Fail "could not read the release from $api"
}

$asset = $release.assets | Where-Object { $_.name -match "_x64-setup\.exe$" } | Select-Object -First 1
if (-not $asset) { Fail "release $($release.tag_name) has no Windows installer." }

Write-Host "Lume $($release.tag_name) · $($asset.name)"
if ($env:LUME_DRY_RUN) {
  Write-Host "[dry run] would download $($asset.browser_download_url)"
  Write-Host "[dry run] would run the installer$(if (-not $env:LUME_INTERACTIVE) { ' silently (/S)' })"
  return
}

$target = Join-Path ([IO.Path]::GetTempPath()) $asset.name
Write-Host "Downloading…"
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $target -UseBasicParsing
Write-Host "SHA-256: $((Get-FileHash -Algorithm SHA256 -Path $target).Hash.ToLower())"

try {
  if ($env:LUME_INTERACTIVE) { Start-Process -FilePath $target -Wait }
  else { Start-Process -FilePath $target -ArgumentList "/S" -Wait }
} finally {
  Remove-Item -Force -ErrorAction SilentlyContinue $target
}
Write-Host "Lume $($release.tag_name) is installed. Open it from the Start menu."
