# PhysioPulse — speech-to-text on the AI PC (PhysioPulse plan, phase 2).
# Installs whisper.cpp's server and the large-v3-turbo model (quantised, ~550 MB) in
# %LOCALAPPDATA%\PhysioPulse\whisper, starts it on port 8178, and starts it again at every sign-in.
# Nothing here needs administrator rights. Windows may ask once whether to allow it on the network: choose
# "Private networks" and Allow (the clinic server must reach it; it is never published on the internet).
#
# Run in PowerShell:  powershell -ExecutionPolicy Bypass -File .\setup-whisper.ps1
# Run it again to update. To remove: delete the folder above and "PhysioPulse whisper.vbs" in shell:startup.
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$dir = Join-Path $env:LOCALAPPDATA "PhysioPulse\whisper"
$port = 8178
New-Item -ItemType Directory -Force -Path $dir | Out-Null
Write-Host "1/5 Looking for an NVIDIA graphics card..."
$nvidia = $false
try { if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) { nvidia-smi -L | Out-Host; $nvidia = $true } } catch {}
if (-not $nvidia) {
  $gpus = (Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name) -join ", "
  Write-Host "   Graphics: $gpus"
  if ($gpus -match "NVIDIA") { $nvidia = $true }
}

Write-Host "2/5 Downloading whisper.cpp (newest release that has a Windows build)..."
# The newest release sometimes has no files yet (they're added after), so look through the last few.
$releases = Invoke-RestMethod -UseBasicParsing -Headers @{ "User-Agent" = "PhysioPulse" } "https://api.github.com/repos/ggml-org/whisper.cpp/releases?per_page=10"
$rel = $null; $pick = $null
foreach ($r in $releases) {
  if ($r.draft -or $r.prerelease) { continue }
  $assets = @($r.assets | Where-Object { $_.name -like "*x64*.zip" -and $_.name -notlike "*arm*" })
  if (-not $assets.Count) { continue }
  if ($nvidia) { $pick = $assets | Where-Object { $_.name -match "cublas|cuda" } | Sort-Object { if ($_.name -match "12") { 0 } else { 1 } } | Select-Object -First 1 }
  if (-not $pick) { $pick = $assets | Where-Object { $_.name -match "^whisper-bin-x64" } | Select-Object -First 1 }
  if (-not $pick) { $pick = $assets | Where-Object { $_.name -match "blas" -and $_.name -notmatch "cublas" } | Select-Object -First 1 }
  if ($pick) { $rel = $r; break }
}
if (-not $pick) { throw "No Windows build found in the last 10 whisper.cpp releases." }
Write-Host "   $($rel.tag_name): $($pick.name)"
$zip = Join-Path $dir $pick.name
curl.exe -L --fail -o $zip $pick.browser_download_url
if (Test-Path "$dir\bin") { Remove-Item -Recurse -Force "$dir\bin" }
Expand-Archive -Force $zip "$dir\bin"
Remove-Item $zip
$server = Get-ChildItem -Recurse "$dir\bin" -Filter "whisper-server.exe" | Select-Object -First 1
if (-not $server) { throw "whisper-server.exe isn't in $($pick.name)." }
Write-Host "   $($server.FullName)"

Write-Host "3/5 Downloading the model (large-v3-turbo, quantised, ~550 MB; once)..."
$model = Join-Path $dir "ggml-large-v3-turbo-q5_0.bin"
if (-not (Test-Path $model) -or (Get-Item $model).Length -lt 400MB) {
  curl.exe -L --fail -o "$model.part" "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin"
  Move-Item -Force "$model.part" $model
}
Write-Host "   $([math]::Round((Get-Item $model).Length / 1MB)) MB"

Write-Host "4/5 Starting it now and at every sign-in..."
$threads = [Math]::Max(2, [Math]::Min(8, [Environment]::ProcessorCount - 2))
$cmd = "`"$($server.FullName)`" -m `"$model`" --host 0.0.0.0 --port $port -l pt -t $threads"
$launcher = Join-Path $dir "start-whisper.cmd"
Set-Content -Encoding ASCII -Path $launcher -Value @("@echo off", "cd /d `"$($server.DirectoryName)`"", "$cmd > `"$dir\server.log`" 2>&1")
# Started hidden at sign-in, so nothing shows on the desktop.
$vbs = Join-Path ([Environment]::GetFolderPath("Startup")) "PhysioPulse whisper.vbs"
Set-Content -Encoding ASCII -Path $vbs -Value ('CreateObject("WScript.Shell").Run """' + $launcher + '""", 0, False')
Get-Process whisper-server -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Process wscript.exe -ArgumentList "`"$vbs`""
Start-Sleep -Seconds 8

Write-Host "5/5 Checking it answers on port $port..."
$ok = $false
for ($i = 0; $i -lt 20 -and -not $ok; $i++) {
  try { Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$port/" -TimeoutSec 3 | Out-Null; $ok = $true } catch { Start-Sleep -Seconds 3 }
}
if (-not $ok) { Write-Host "It didn't answer. Last lines of $dir\server.log:"; Get-Content "$dir\server.log" -Tail 20; exit 1 }
$ips = (Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notlike "127.*" -and $_.IPAddress -notlike "169.254.*" } | ForEach-Object IPAddress) -join ", "
Write-Host ""
Write-Host "Done. whisper-server answers on port $port. This computer's addresses: $ips"
Write-Host "In PhysioPulse: Settings > Local AI > Speech to text > type this computer's address (e.g. 100.86.97.21) > Test Connection > Save."
