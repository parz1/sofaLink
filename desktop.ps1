param([string]$GameProcess='LostJudgment',[ValidateRange(0,86400)][int]$Seconds=0,[long]$MonitorHandle=0)
$ErrorActionPreference='Stop'
$game=@(Get-Process -Name $GameProcess -ErrorAction SilentlyContinue | Where-Object {$_.MainWindowHandle -ne 0})
if($game.Count -ne 1){throw 'Start the game first; expected exactly one game window'}
$desktopExe=Join-Path $PSScriptRoot 'target/debug/sofalink-desktop.exe'
if(!(Test-Path -LiteralPath $desktopExe)){throw 'Build first: cargo build -p sofalink-desktop'}
$desktopArgs=@('--root',$PSScriptRoot,'--game-pid',"$($game[0].Id)")
if($Seconds){$desktopArgs+=@('--run-seconds',"$Seconds")}
if($MonitorHandle -gt 0){$desktopArgs+=@('--monitor-handle',"$MonitorHandle")}
& $desktopExe @desktopArgs
if($LASTEXITCODE -ne 0){throw 'Desktop focus overlay failed'}
