# PhysioPulse — quick check of speech-to-text on the AI PC (PhysioPulse plan, phase 2).
# Makes a short INVENTED physiotherapy conversation with Windows' own voices (no real patient), sends it to the
# local whisper-server and prints what it understood and how long it took. Nothing leaves this computer.
# Run in PowerShell:  powershell -ExecutionPolicy Bypass -File .\test-whisper.ps1
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Speech
$lines = @(
  "Bom dia. Então, como tem passado do ombro desde a semana passada?",
  "Melhor. A dor à noite já quase não aparece, mas quando levanto o braço acima da cabeça ainda dói, talvez um cinco em dez.",
  "E para vestir o casaco, ou chegar à prateleira de cima?",
  "O casaco já consigo. A prateleira ainda me custa.",
  "Vamos medir. Elevação do braço direito, cento e quarenta graus. Na semana passada eram cento e vinte.",
  "Hoje fazemos mobilização do ombro e reforço dos rotadores, com o elástico verde.",
  "Em casa continue os exercícios duas vezes por dia. Na próxima sessão passamos para o elástico azul."
)
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
$voices = @($s.GetInstalledVoices() | Where-Object { $_.VoiceInfo.Culture.Name -like "pt*" } | ForEach-Object { $_.VoiceInfo.Name })
if (-not $voices.Count) { Write-Host "No Portuguese voice is installed in Windows (Settings > Time & language > Speech), so English voices will read it and the text will be poor. The real check is two people reading claude\RECORDING_TEST.md."; $voices = @($s.GetInstalledVoices() | ForEach-Object { $_.VoiceInfo.Name }) }
$wav = Join-Path $env:TEMP "physiopulse-whisper-test.wav"
$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
$s.SetOutputToWaveFile($wav, $fmt)
for ($i = 0; $i -lt $lines.Count; $i++) { $s.SelectVoice($voices[$i % $voices.Count]); $s.Speak($lines[$i]) }
$s.Dispose()
Write-Host "Voices: $($voices -join ', ')  Audio: $([math]::Round((Get-Item $wav).Length / 32000)) s"
$t = [Diagnostics.Stopwatch]::StartNew()
$out = curl.exe -s -F "file=@$wav" -F "language=pt" -F "response_format=json" -F "temperature=0.0" "http://127.0.0.1:8178/inference"
$t.Stop()
Remove-Item $wav
Write-Host "Took $([math]::Round($t.Elapsed.TotalSeconds, 1)) s. Text:"
($out | ConvertFrom-Json).text
