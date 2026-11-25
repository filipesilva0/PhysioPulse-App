# Publishes a PhysioPulse release from this folder (claude/RELEASING.md). Run on the owner's Windows PC,
# by Claude through Remote Control or by hand:
#
#   powershell -ExecutionPolicy Bypass -File tools\release\publish.ps1 -Version 0.15.0
#
# 1. Checks claude/releases/v<Version>.md exists (what changed; its first paragraph is shown in the app).
# 2. Sets the version in every file (tools/release/bump.mjs).
# 3. Refuses to go on if anything secret-looking would be committed (the repository is PUBLIC).
# 4. Commits everything, tags v<Version> and pushes. GitHub Actions then builds the Mac and Windows apps,
#    signs the updates and publishes the release (.github/workflows/release.yml), in about 20-30 minutes.
#
# The first run also turns this folder into a Git repository connected to GitHub.
param([Parameter(Mandatory = $true)][string]$Version)
$ErrorActionPreference = "Stop"
$Version = $Version.TrimStart("v")
$repo = "filipesilva0/PhysioPulse-App"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $root

function G { & git.exe @args; if ($LASTEXITCODE -ne 0) { throw "git $args failed" } }

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must be X.Y.Z, e.g. 0.15.0" }
$notes = "claude\releases\v$Version.md"
if (-not (Test-Path $notes)) { throw "Write $notes first (what changed, in short)." }
if ((Get-Content src-tauri\tauri.conf.json -Raw) -match "REPLACE_WITH_UPDATER_PUBLIC_KEY") {
  throw "No updater public key yet: run tools\release\new-signing-key.ps1 once first."
}

# A Git repository connected to GitHub (first run only).
if (-not (Test-Path ".git")) {
  G init -b main
  G remote add origin "https://github.com/$repo.git"
}
if (-not (git.exe config user.name)) { G config user.name "PhysioPulse" }
if (-not (git.exe config user.email)) { G config user.email "108900101+filipesilva0@users.noreply.github.com" }
if (git.exe tag --list "v$Version") { throw "v$Version was already released. Choose the next version." }

# Version everywhere (skipped when it's already this version).
$current = (Get-Content package.json -Raw | ConvertFrom-Json).version
if ($current -ne $Version) {
  node tools/release/bump.mjs $Version
  if ($LASTEXITCODE -ne 0) { throw "bump failed" }
}

# The workflow lives in tools/release/release.yml (Claude's file tools can't write inside .github); GitHub needs it
# in .github/workflows, so it's copied there whenever it differs.
$wf = ".github\workflows\release.yml"
if (-not (Test-Path $wf) -or (Get-FileHash tools\release\release.yml).Hash -ne (Get-FileHash $wf).Hash) {
  New-Item -ItemType Directory -Force -Path ".github\workflows" | Out-Null
  Copy-Item tools\release\release.yml $wf -Force
}

# Stage everything .gitignore allows, then check it before anything leaves the computer.
G add -A
$files = @(git.exe -c core.quotepath=false diff --cached --name-only)
$bad = @()
# Written in pieces so this file doesn't match itself. The updater key file is base64 of "untrusted comment: rsign
# encrypted secret key", so both spellings are checked.
$secretRe = ('BEGIN [A-Z ]*PRIV' + 'ATE KEY|rsign encrypted ' + 'secret key|dW50cnVzdGVkIGNvbW1lbnQ6IHJzaWdu' + 'IGVuY3J5cHRlZCBzZWNyZXQga2V5|' +
  'ghp_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|sk-ant-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}')
foreach ($f in $files) {
  if (-not (Test-Path -LiteralPath $f)) { continue }   # deleted
  $leaf = Split-Path $f -Leaf
  if ($leaf -match '\.(key|pem|p12|pfx|age)$' -or $leaf -eq ".env" -or $leaf -match '^\.env\.(?!example$)') { $bad += "$f (secret file type)"; continue }
  $item = Get-Item -LiteralPath $f
  if ($item.Length -gt 50MB) { $bad += "$f (over 50 MB)"; continue }
  if ($item.Length -lt 2MB -and $f -notmatch '\.(png|jpg|jpeg|ico|icns|woff2|ttf|bmp|gif|webp|pdf|exe|zip)$') {
    $t = Get-Content -LiteralPath $f -Raw -ErrorAction SilentlyContinue
    if ($t -match $secretRe) { $bad += "$f (looks like it holds a secret)" }
  }
}
if ($bad.Count) {
  G reset -q
  Write-Host "Stopped: these would be published in the public repository:" -ForegroundColor Red
  $bad | ForEach-Object { Write-Host "  $_" }
  Write-Host "Add them to .gitignore (or remove the secret) and run again."
  exit 1
}

G commit -q -m "Release v$Version" -m "See claude/releases/v$Version.md"
G tag -a "v$Version" -m "PhysioPulse $Version"
G push -u origin main
G push origin "v$Version"
Write-Host ""
Write-Host "Pushed v$Version. Build and release: https://github.com/$repo/actions" -ForegroundColor Green
Write-Host "When it's green, every PhysioPulse sees the update (Settings > Overview > Check for Updates)."
