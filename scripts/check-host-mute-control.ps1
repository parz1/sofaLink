$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$script=Join-Path $PSScriptRoot 'audio-mute.ps1'
$statePath=Join-Path $root 'local/audio-mute.json'
if(Test-Path $statePath){throw 'End or recover the existing mute session first'}
if(@(Get-NetTCPConnection -LocalPort 5004,5006 -State Listen -ErrorAction SilentlyContinue).Count){throw 'End the active stream first'}
$before=& $script -Action Inspect | ConvertFrom-Json
$defaults=@(0,1,2 | ForEach-Object {[Livingroom.Audio.Endpoints]::Default($_)})
$session='mute-check-'+[Guid]::NewGuid().ToString('N')
$owner=$null
try {
    foreach($initial in @($false,$true)) {
        [Livingroom.Audio.Endpoints]::Mute($before.endpoint,$initial)
        & $script -Action Acquire -OwnerPid $PID -Session $session | Out-Null
        if(-not [Livingroom.Audio.Endpoints]::Muted($before.endpoint)){throw 'Mute was not applied'}
        $rejected=$false
        try {& $script -Action Release -OwnerPid $PID -Session 'wrong-owner' | Out-Null} catch {$rejected=$true}
        if(-not $rejected){throw 'Foreign release was accepted'}
        & $script -Action Release -OwnerPid $PID -Session $session | Out-Null
        if([Livingroom.Audio.Endpoints]::Muted($before.endpoint) -ne $initial){throw 'Original mute state was not restored'}
    }
    # Preserve a deliberate user unmute, even when the session began muted.
    & $script -Action Acquire -OwnerPid $PID -Session $session | Out-Null
    [Livingroom.Audio.Endpoints]::Mute($before.endpoint,$false)
    & $script -Action Release -OwnerPid $PID -Session $session | Out-Null
    if([Livingroom.Audio.Endpoints]::Muted($before.endpoint)){throw 'User unmute was overwritten'}
    $owner=Start-Process powershell.exe -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 60') -PassThru -WindowStyle Hidden
    & $script -Action Acquire -OwnerPid $owner.Id -Session ($session+'-crash') | Out-Null
    Stop-Process -Id $owner.Id;$owner.WaitForExit()
    $deadline=[DateTime]::UtcNow.AddSeconds(10)
    while((Test-Path $statePath) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 100}
    if((Test-Path $statePath) -or [Livingroom.Audio.Endpoints]::Muted($before.endpoint)){throw 'Guardian did not automatically restore after owner termination'}
} finally {
    if($null -ne $owner -and -not $owner.HasExited){$owner.Kill();$owner.WaitForExit()}
    if(Test-Path $statePath){
        $state=Get-Content $statePath -Raw | ConvertFrom-Json
        if($state.ownerPid -eq $PID){& $script -Action Release -OwnerPid $PID -Session $session | Out-Null}
        else {& $script -Action Recover | Out-Null}
    }
    [Livingroom.Audio.Endpoints]::Mute($before.endpoint,$before.muted)
    if([Math]::Abs([double][Livingroom.Audio.Endpoints]::Level($before.endpoint)-[double]$before.volume) -gt 0.000001){throw 'Volume changed'}
    if((@(0,1,2 | ForEach-Object {[Livingroom.Audio.Endpoints]::Default($_)}) -join '|') -ne ($defaults -join '|')){throw 'Default devices changed'}
}
Write-Output 'PASS: mute, initially muted/unmuted restore, foreign owner rejection, user unmute, automatic forced-exit recovery; defaults and volume unchanged'
