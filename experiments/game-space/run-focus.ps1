param([Parameter(Mandatory)][string]$Device,[Parameter(Mandatory)][string]$Python,[ValidateRange(30,86400)][int]$Seconds=600,[switch]$NoOverlay,[switch]$Managed,[string]$StopFile,[int]$OwnerPid=0,[string]$OwnerCreated)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$dir=Join-Path $root 'local/game-space'
function Health($state){
    if($Managed -and $StopFile){
        $healthFile=$StopFile+'.health.json';$temporary=$healthFile+'.tmp'
        @{state=$state;updated_ms=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()} | ConvertTo-Json | Set-Content -LiteralPath $temporary -Encoding UTF8
        Move-Item -LiteralPath $temporary -Destination $healthFile -Force
    }
}
function Cancelled {
    if($StopFile -and (Test-Path -LiteralPath $StopFile)){return $true}
    if($OwnerPid){$owner=Get-Process -Id $OwnerPid -ErrorAction SilentlyContinue;if(!$owner -or ($OwnerCreated -and $owner.StartTime.ToUniversalTime().Ticks -ne ([DateTime]$OwnerCreated).ToUniversalTime().Ticks)){return $true}}
    $false
}
if(Cancelled){throw 'Session owner has exited or requested stop'}
$adb=Join-Path $root 'local/tools/platform-tools/adb.exe'
$serial=if($Device.Contains(':')){$Device}else{"${Device}:5555"}
$deviceIp=$serial.Split(':')[0]
& $adb connect $serial
if($LASTEXITCODE -ne 0){throw 'Fire TV ADB connection failed'}
& $adb -s $serial get-state
if($LASTEXITCODE -ne 0){throw 'Fire TV is unavailable; turn on the device before streaming'}
$route=[Net.Sockets.UdpClient]::new();try{$route.Connect($deviceIp,5004);$hostIp=$route.Client.LocalEndPoint.Address.ToString()}finally{$route.Dispose()}
if(Test-Path -LiteralPath (Join-Path $root 'local/host-runtime.json')){
    $hostRecord=Get-Content -LiteralPath (Join-Path $root 'local/host-runtime.json') -Raw | ConvertFrom-Json
    $stateFile=Join-Path $root "local/host-state-$($hostRecord.pid).jsonl"
    if((Get-Process -Id $hostRecord.pid -ErrorAction SilentlyContinue) -and (Test-Path -LiteralPath $stateFile)){
        $state=Get-Content -LiteralPath $stateFile -Tail 1 | ConvertFrom-Json
        if($state.snapshot.phase -ne 'idle'){throw 'End the existing core session before starting a standalone experiment'}
    }
}
$displays=& "$PSScriptRoot/display-probe.ps1" | ConvertFrom-Json
$virtual=@($displays.displays | Where-Object {!$_.primary -and $_.width -eq 1920 -and $_.height -eq 1080})
if($virtual.Count -ne 1){throw 'Expected one active non-primary 1080p virtual display; install the experimental driver first'}
$game=Get-Process LostJudgment -ErrorAction SilentlyContinue
if(!$game){
    $steamExe=(Get-ItemProperty 'HKCU:\Software\Valve\Steam').SteamExe
    Start-Process -FilePath $steamExe -ArgumentList 'steam://rungameid/2058190' -WindowStyle Normal
}
$deadline=[DateTime]::UtcNow.AddSeconds(45)
do{if(Cancelled){throw 'Game startup cancelled'};$game=Get-Process LostJudgment -ErrorAction SilentlyContinue;if($game -and $game.MainWindowHandle -ne 0){break};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow -lt $deadline)
if(!$game -or $game.MainWindowHandle -eq 0){throw 'Game window startup was not confirmed'}
$desktopExe=Join-Path $root 'target/debug/sofalink-desktop.exe'
if(!$NoOverlay -and !(Test-Path -LiteralPath $desktopExe)){throw 'Build the desktop executable first'}
$session="focus-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())"
$hostLog=Join-Path $dir "$session.jsonl"
$stream=$null;$desktopProcess=$null;$moved=$false;$clientStarted=$false
try{
    if(Cancelled){throw 'Session startup cancelled'}
    & "$PSScriptRoot/game-window.ps1" -Action Move -MonitorHandle $virtual[0].handle | Out-Null
    $moved=$true
    $startInfo=[Diagnostics.ProcessStartInfo]::new($Python)
    $startInfo.UseShellExecute=$false;$startInfo.CreateNoWindow=$true;$startInfo.WorkingDirectory=$root;$startInfo.RedirectStandardInput=$true
    foreach($argument in @((Join-Path $root 'experiments/stream/host.py'),'--device',$deviceIp,'--monitor-handle',"$($virtual[0].handle)",'--audio-include-pid',"$($game.Id)",'--diagnostics','--control-stdin','--seconds',"$Seconds",'--session-id',$session,'--log',$hostLog)){[void]$startInfo.ArgumentList.Add($argument)}
    $stream=[Diagnostics.Process]::Start($startInfo)
    $deadline=[DateTime]::UtcNow.AddSeconds(8)
    do{
        if(Cancelled){throw 'Media startup cancelled'}
        if($stream.HasExited){throw 'Media host exited during startup'}
        if(Test-Path -LiteralPath $hostLog){$ready=Get-Content -LiteralPath $hostLog -Tail 20 | Where-Object {($_ | ConvertFrom-Json).type -eq 'host_ready'};if($ready){break}}
        Start-Sleep -Milliseconds 100
    }while([DateTime]::UtcNow -lt $deadline)
    if(!$ready){throw 'Media host readiness was not confirmed'}
    & $adb -s $serial shell am force-stop dev.livingroom.streamlab
    & $adb -s $serial shell am start -n dev.livingroom.streamlab/.MainActivity --es host $hostIp --es transport tcp --ei port 5004 --es sessionId $session --es audioBackend auto --ez diagnostics true --ez syncVideo true --es presentationMode vsync --ei videoQueueFrames 4 --es decoder OMX.MTK.VIDEO.DECODER.AVC --ez requireLowLatencyAudio true
    if($LASTEXITCODE -ne 0){throw 'TV client start failed'};$clientStarted=$true
    if(!$NoOverlay){$desktopProcess=Start-Process -FilePath $desktopExe -WindowStyle Hidden -ArgumentList @('--root',('"'+$root+'"'),'--game-pid',"$($game.Id)",'--monitor-handle',"$($virtual[0].handle)",'--run-seconds',"$Seconds") -PassThru}
    $selfProcess=Get-Process -Id $PID
    @{session=$session;host_pid=$stream.Id;desktop_pid=if($desktopProcess){$desktopProcess.Id}else{0};runner_pid=$PID;runner_created=$selfProcess.StartTime.ToUniversalTime().ToString('o');stop_file=$StopFile;game_pid=$game.Id;monitor_handle=$virtual[0].handle;host_log=$hostLog} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $dir 'focus-runtime.json') -Encoding UTF8
    $deadline=[DateTime]::UtcNow.AddSeconds($Seconds)
    Health 'running'
    $lastFrames=-1;$progress=[DateTime]::UtcNow;$nextCheck=[DateTime]::UtcNow
    while((!$desktopProcess -or !$desktopProcess.HasExited) -and !$stream.HasExited -and [DateTime]::UtcNow -lt $deadline -and (Get-Process -Id $game.Id -ErrorAction SilentlyContinue) -and !(Cancelled)){
        if([DateTime]::UtcNow -ge $nextCheck){
            $samples=@(Get-Content -LiteralPath $hostLog -Tail 240 | ForEach-Object {try{$_ | ConvertFrom-Json}catch{}} | Where-Object {$_.type -eq 'client'})
            if($samples.Count){$metrics=$samples[-1].metrics;if($metrics.rendered -gt $lastFrames -and $metrics.videoHealthy -and $metrics.audioHealthy){$progress=[DateTime]::UtcNow};$lastFrames=$metrics.rendered}
            if(([DateTime]::UtcNow-$progress).TotalSeconds -gt 5){throw 'TV audio/video progress stalled'}
            Health 'running'
            $nextCheck=[DateTime]::UtcNow.AddSeconds(1)
        }
        Start-Sleep -Milliseconds 200;if($desktopProcess){$desktopProcess.Refresh()};$stream.Refresh()
    }
}finally{
    Health 'stopping'
    if($stream -and !$stream.HasExited){$stream.StandardInput.WriteLine('stop');$stream.StandardInput.Flush();if(!$stream.WaitForExit(5000)){$stream.Kill();$stream.WaitForExit()}}
    if($desktopProcess -and !$desktopProcess.HasExited){$desktopProcess.CloseMainWindow() | Out-Null;if(!$desktopProcess.WaitForExit(3000)){$desktopProcess.Kill()}}
    if($clientStarted){& $adb -s $serial shell am force-stop dev.livingroom.streamlab;& $adb -s $serial shell am start -n dev.livingroom.streamlab/.LauncherActivity}
    if(!$Managed -and $moved -and (Get-Process -Id $game.Id -ErrorAction SilentlyContinue)){& "$PSScriptRoot/game-window.ps1" -Action Restore | Out-Null}
}
