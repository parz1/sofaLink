param([ValidateSet('inspect','create_display','start_media','stop_media','restore_game','remove_display','uninstall')][string]$Action='inspect',[Parameter(Mandatory)][string]$ResultFile,[string]$Device,[string]$Python,[string]$Shell,[int]$OwnerPid=0)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$dir=Join-Path $root 'local/game-space'
$leaseFile=Join-Path $dir 'display-driver-lease.json'
$runtimeFile=Join-Path $dir 'focus-runtime.json'
$runnerFile=Join-Path $dir 'managed-runner.json'
$windowLease=Join-Path $dir 'game-window-lease.json'
$errorMessage=$null
$operationLock=[Threading.Mutex]::new($false,'Global\sofaLink-DisplayOperation')
$lockAcquired=$false
function Runtime {
    if(!(Test-Path -LiteralPath $runtimeFile)){return $null}
    Get-Content -LiteralPath $runtimeFile -Raw | ConvertFrom-Json
}
function OwnedRunner {
    $managed=if(Test-Path -LiteralPath $runnerFile){Get-Content -LiteralPath $runnerFile -Raw | ConvertFrom-Json}else{$null}
    if(RunningRunner $managed){return $managed}
    $runtime=Runtime
    if(RunningRunner $runtime){return $runtime}
    $managed
}
function RunningRunner($runtime){
    if(!$runtime -or !$runtime.runner_pid -or !$runtime.runner_created){return $false}
    $process=Get-Process -Id $runtime.runner_pid -ErrorAction SilentlyContinue
    $process -and $process.StartTime.ToUniversalTime().Ticks -eq ([DateTime]$runtime.runner_created).ToUniversalTime().Ticks
}
function Inspect {
    $display='stopped'
    if(Test-Path -LiteralPath $leaseFile){
        $lease=Get-Content -LiteralPath $leaseFile -Raw | ConvertFrom-Json
        $display=if($lease.instance -and (Get-PnpDevice -InstanceId $lease.instance -PresentOnly -ErrorAction SilentlyContinue)){'running'}else{'unknown'}
    }
    $runtime=OwnedRunner
    $media=if(RunningRunner $runtime){'running'}else{'stopped'}
    $window=if(Test-Path -LiteralPath $windowLease){'unknown'}else{'stopped'}
    if($media -eq 'running' -and $window -eq 'unknown'){$window='running'}
    $source=Join-Path $dir 'vdd-control/SignedDrivers/x86/VDD/MttVDD.inf'
    $driver=$false
    if(Test-Path -LiteralPath $source){$hash=(Get-FileHash -LiteralPath $source).Hash;$driver=@(Get-ChildItem -LiteralPath "$env:windir\INF" -Filter 'oem*.inf' | Where-Object {(Get-FileHash -LiteralPath $_.FullName).Hash -eq $hash}).Count -gt 0}
    @{display=$display;media=$media;game_window=$window;driver_installed=$driver}
}
try{
    try{$lockAcquired=$operationLock.WaitOne(50000)}catch [Threading.AbandonedMutexException]{$lockAcquired=$true}
    if(!$lockAcquired){throw '上一个系统操作尚未完成，请等待授权结束后恢复'}
    switch($Action){
        {$_ -in @('create_display','remove_display','uninstall')} {
            $operation=@{create_display='Install';remove_display='Remove';uninstall='Uninstall'}[$Action]
            if($Action -eq 'create_display'){
                $adb=Join-Path $root 'local/tools/platform-tools/adb.exe'
                $serial=if($Device.Contains(':')){$Device}else{"${Device}:5555"}
                & $adb connect $serial | Out-Null
                & $adb -s $serial get-state *> (Join-Path $dir 'tv-preflight.log')
                if($LASTEXITCODE -ne 0){throw 'Fire TV 未连接或未授权，请先确认电视调试授权'}
            }
            if($Action -eq 'remove_display' -and !(Test-Path -LiteralPath $leaseFile)){break}
            $record=Join-Path $dir 'driver-operation.json'
            if(Test-Path -LiteralPath $record){Remove-Item -LiteralPath $record}
            $process=Start-Process powershell.exe -Verb RunAs -WindowStyle Hidden -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',('"'+(Join-Path $PSScriptRoot 'display-driver.ps1')+'"'),'-Action',$operation) -PassThru -Wait -ErrorAction Stop
            if(!(Test-Path -LiteralPath $record)){throw 'Elevated operation did not return evidence'}
            $result=Get-Content -LiteralPath $record -Raw | ConvertFrom-Json
            if(!$result.success -or $result.action -ne $operation){throw $result.error}
        }
        'start_media' {
            if(RunningRunner (OwnedRunner)){throw 'An owned media session is already running'}
            $stopFile=Join-Path $dir "stop-$([Guid]::NewGuid().ToString('N')).request"
            $ownerCreated=(Get-Process -Id $OwnerPid).StartTime.ToUniversalTime().ToString('o')
            $runner=Start-Process -FilePath $Shell -WindowStyle Hidden -ArgumentList @('-NoProfile','-File',('"'+(Join-Path $PSScriptRoot 'run-focus.ps1')+'"'),'-Device',$Device,'-Python',('"'+$Python+'"'),'-Seconds','86400','-NoOverlay','-Managed','-StopFile',('"'+$stopFile+'"'),'-OwnerPid',"$OwnerPid",'-OwnerCreated',$ownerCreated) -RedirectStandardOutput (Join-Path $dir 'managed-media.stdout') -RedirectStandardError (Join-Path $dir 'managed-media.stderr') -PassThru
            @{runner_pid=$runner.Id;runner_created=$runner.StartTime.ToUniversalTime().ToString('o');stop_file=$stopFile} | ConvertTo-Json | Set-Content -LiteralPath $runnerFile -Encoding UTF8
            $deadline=[DateTime]::UtcNow.AddSeconds(75);$lastFrames=-1;$confirmed=$false
            do{
                $runner.Refresh();if($runner.HasExited){throw 'Media runner failed; see managed-media.stderr'}
                $runtime=Runtime
                if($runtime -and $runtime.runner_pid -eq $runner.Id -and (Test-Path -LiteralPath $runtime.host_log)){
                    $samples=@(Get-Content -LiteralPath $runtime.host_log -Tail 240 | ForEach-Object {try{$_ | ConvertFrom-Json}catch{}} | Where-Object {$_.type -eq 'client'})
                    if($samples.Count){$metrics=$samples[-1].metrics;if($metrics.videoHealthy -and $metrics.audioHealthy -and $metrics.audioVideoSyncEnabled -and $metrics.rendered -gt $lastFrames -and $lastFrames -ge 0){$confirmed=$true;break};$lastFrames=$metrics.rendered}
                }
                Start-Sleep -Milliseconds 250
            }while([DateTime]::UtcNow -lt $deadline)
            if(!$confirmed){throw 'Real TV audio/video progress was not confirmed'}
        }
        'stop_media' {
            $runtime=OwnedRunner
            if(RunningRunner $runtime){
                $stopPath=[IO.Path]::GetFullPath($runtime.stop_file)
                if(!$stopPath.StartsWith(($dir+[IO.Path]::DirectorySeparatorChar),[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($stopPath) -notmatch '^stop-[a-f0-9]{32}\.request$'){throw 'Unexpected stop-file identity'}
                Set-Content -LiteralPath $stopPath -Value 'stop' -Encoding UTF8
                Wait-Process -Id $runtime.runner_pid -Timeout 15 -ErrorAction Stop
            }
        }
        'restore_game' {
            if(Test-Path -LiteralPath $windowLease){
                $saved=Get-Content -LiteralPath $windowLease -Raw | ConvertFrom-Json
                $game=Get-Process -Id $saved.pid -ErrorAction SilentlyContinue
                if($game -and $game.StartTime.ToUniversalTime().Ticks -eq ([DateTime]$saved.process_start).ToUniversalTime().Ticks){& "$PSScriptRoot/game-window.ps1" -Action Restore | Out-Null}
                else{Move-Item -LiteralPath $windowLease -Destination (Join-Path $dir "expired-window-$([Guid]::NewGuid().ToString('N')).json")}
            }
        }
    }
}catch{$errorMessage=$_.Exception.Message;if($errorMessage -match 'cancel|取消'){$errorMessage='Windows 授权已取消；请恢复或重试'}}
try{if(!$lockAcquired){throw 'Operation lock not acquired'};$resources=Inspect}catch{$resources=@{display='unknown';media='unknown';game_window='unknown';driver_installed=$true};if(!$errorMessage){$errorMessage=$_.Exception.Message}}
$runtime=Runtime
@{resources=$resources;error=$errorMessage;binding=if($runtime -and $resources.media -eq 'running'){@{game_pid=$runtime.game_pid;monitor=$runtime.monitor_handle;session=$runtime.session;host_log=$runtime.host_log;health_file=$runtime.stop_file+'.health.json'}}else{$null}} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $ResultFile -Encoding UTF8
if($lockAcquired){$operationLock.ReleaseMutex()};$operationLock.Dispose()
