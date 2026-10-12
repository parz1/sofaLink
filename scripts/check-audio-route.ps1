# Actual Windows endpoint tests. All mutations are restored in finally.
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$route=Join-Path $PSScriptRoot 'audio-route.ps1'
if(Test-Path (Join-Path $root 'local/audio-route.json')){throw 'End or recover the existing audio session before testing'}
if(@(Get-NetTCPConnection -LocalPort 5004,5006 -State Listen -ErrorAction SilentlyContinue).Count){throw 'End the active stream before testing audio routing'}
$before=& $route -Action Inspect | ConvertFrom-Json
$session='audio-route-check-'+[Guid]::NewGuid().ToString('N')
$owner=$null
try {
    $acquired=& $route -Action Acquire -OwnerPid $PID -Session $session | ConvertFrom-Json
    if(@($acquired.defaults | Where-Object {$_ -ne $before.steamEndpoint}).Count){throw 'Virtual sink was not adopted'}
    $rejected=$false
    try { & $route -Action Release -OwnerPid $PID -Session 'wrong-owner' | Out-Null } catch {$rejected=$true}
    if(-not $rejected){throw 'Foreign release was accepted'}
    $rejected=$false
    try { & $route -Action Recover | Out-Null } catch {$rejected=$true}
    if(-not $rejected){throw 'Live session recovery was accepted'}
    # A user changing the console device must not be overwritten at release.
    [Livingroom.Audio.Endpoints]::Set($before.defaults[0],0)
    & $route -Action Release -OwnerPid $PID -Session $session | Out-Null
    $after=& $route -Action Inspect | ConvertFrom-Json
    if(($after.defaults -join '|') -ne ($before.defaults -join '|')){throw 'Normal release did not restore original endpoints'}
    $owner=Start-Process -FilePath powershell.exe -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 60') -PassThru -WindowStyle Hidden
    & $route -Action Acquire -OwnerPid $owner.Id -Session $session | Out-Null
    Stop-Process -Id $owner.Id -ErrorAction Stop
    $owner.WaitForExit()
    & $route -Action Recover | Out-Null
    $after=& $route -Action Inspect | ConvertFrom-Json
    if(($after.defaults -join '|') -ne ($before.defaults -join '|') -or $null -ne $after.lease){throw 'Crash recovery failed'}
    Write-Output 'PASS: actual switch, owner checks, user change preservation, normal restore and dead-owner recovery'
} finally {
    if($null -ne $owner -and -not $owner.HasExited){$owner.Kill();$owner.WaitForExit()}
    if(Test-Path (Join-Path $root 'local/audio-route.json')) {
        $lease=Get-Content (Join-Path $root 'local/audio-route.json') -Raw | ConvertFrom-Json
        if($lease.ownerPid -eq $PID){& $route -Action Release -OwnerPid $PID -Session $session | Out-Null}
        else {& $route -Action Recover | Out-Null}
    }
}
