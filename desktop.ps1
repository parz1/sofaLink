param([string]$GameProcess='LostJudgment',[ValidateRange(0,86400)][int]$Seconds=0,[long]$MonitorHandle=0,[switch]$FocusOnly,[string]$Device,[string]$Python,[string]$Shell)
$ErrorActionPreference='Stop'
$game=@(Get-Process -Name $GameProcess -ErrorAction SilentlyContinue | Where-Object {$_.MainWindowHandle -ne 0})
if($FocusOnly -and $game.Count -ne 1){throw 'Start the game first; expected exactly one game window'}
$desktopExe=Join-Path $PSScriptRoot 'target/debug/sofalink-desktop.exe'
if(!(Test-Path -LiteralPath $desktopExe)){throw 'Build first: cargo build -p sofalink-desktop'}
$desktopArgs=@('--root',$PSScriptRoot)
if($game.Count -eq 1){$desktopArgs+=@('--game-pid',"$($game[0].Id)")}
if(!$FocusOnly){
    if(!$Device -and (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'config.local.json'))){$Device=(Get-Content (Join-Path $PSScriptRoot 'config.local.json') -Raw | ConvertFrom-Json).device}
    if(!$Device){throw 'Provide -Device with the paired Fire TV IP'}
    if(!$Python -and (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'local/emulator-runtime.json'))){$Python=(Get-Content (Join-Path $PSScriptRoot 'local/emulator-runtime.json') -Raw | ConvertFrom-Json).python}
    if(!$Python){throw 'Provide -Python with the prepared GStreamer runtime'}
    if(!$Shell -and $PSVersionTable.PSVersion.Major -ge 7){$Shell=(Get-Process -Id $PID).Path}
    if(!$Shell){throw 'Run in PowerShell 7 or provide -Shell'}
    $desktopArgs+=@('--manage-space','--device',$Device,'--python',$Python,'--shell',$Shell)
}
if($Seconds){$desktopArgs+=@('--run-seconds',"$Seconds")}
if($MonitorHandle -gt 0){$desktopArgs+=@('--monitor-handle',"$MonitorHandle")}
& $desktopExe @desktopArgs
if($LASTEXITCODE -ne 0){throw 'Desktop focus overlay failed'}
