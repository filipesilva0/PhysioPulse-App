# PhysioPulse updater key: run ONCE, on the owner's Windows PC (claude/RELEASING.md).
#
#   Right-click > Run with PowerShell, or:  powershell -ExecutionPolicy Bypass -File tools\release\new-signing-key.ps1
#
# 1. Makes the key pair with the Tauri CLI (it asks you for a password: choose one and keep it).
#    The private key goes to  %USERPROFILE%\.physiopulse\updater.key  (outside this folder, never in Git).
# 2. Puts the PUBLIC key into src-tauri\tauri.conf.json (plugins.updater.pubkey). Every app built after this
#    accepts only updates signed with your private key.
# 3. Copies the PRIVATE key to the clipboard and opens the repository's Actions secrets page, so you can paste
#    it as TAURI_SIGNING_PRIVATE_KEY (and the password as TAURI_SIGNING_PRIVATE_KEY_PASSWORD).
#
# Keep a copy of updater.key and its password somewhere safe (a password manager). If they are lost, installed
# apps can no longer be updated automatically: each computer would need one manual install of a build with a new key.
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$conf = Join-Path $root "src-tauri\tauri.conf.json"
$dir = Join-Path $env:USERPROFILE ".physiopulse"
$key = Join-Path $dir "updater.key"
$repo = "filipesilva0/PhysioPulse-App"

if (Test-Path $key) {
  Write-Host "There is already a key at $key. Not making a new one (it would lock out the copies already installed)." -ForegroundColor Yellow
  Write-Host "Delete it by hand first only if you are sure."
  exit 1
}
New-Item -ItemType Directory -Force -Path $dir | Out-Null

Push-Location $root
try {
  Write-Host "Making the updater key. Choose a password when asked, and keep it." -ForegroundColor Cyan
  npx tauri signer generate -w "$key"
  if ($LASTEXITCODE -ne 0) { throw "tauri signer generate failed" }
} finally { Pop-Location }

$pub = (Get-Content "$key.pub" -Raw).Trim()
$text = [IO.File]::ReadAllText($conf)
if ($text -notmatch '"pubkey"\s*:\s*"[^"]*"') { throw "No plugins.updater.pubkey in $conf" }
$text = [regex]::Replace($text, '"pubkey"\s*:\s*"[^"]*"', '"pubkey": "' + $pub + '"')
[IO.File]::WriteAllText($conf, $text, (New-Object Text.UTF8Encoding $false))
Write-Host "Public key written to src-tauri\tauri.conf.json." -ForegroundColor Green

Set-Clipboard -Value ((Get-Content $key -Raw).Trim())
Write-Host ""
Write-Host "The PRIVATE key is now on the clipboard." -ForegroundColor Green
Write-Host "In the page that opens: New repository secret"
Write-Host "  Name: TAURI_SIGNING_PRIVATE_KEY            Secret: paste (Ctrl+V)"
Write-Host "Then another one:"
Write-Host "  Name: TAURI_SIGNING_PRIVATE_KEY_PASSWORD   Secret: the password you just chose"
Start-Process "https://github.com/$repo/settings/secrets/actions/new"
Write-Host ""
Write-Host "Back up $key and the password (password manager). Then tell Claude the key is done."
