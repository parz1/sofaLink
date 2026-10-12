param(
    [ValidateSet('Inspect','Acquire','Release','Recover','Watch')][string]$Action='Inspect',
    [int]$OwnerPid=0,
    [ValidatePattern('^[a-zA-Z0-9_-]{1,100}$')][string]$Session='manual'
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$statePath=Join-Path $root 'local/audio-mute.json'
New-Item -ItemType Directory -Force (Join-Path $root 'local') | Out-Null
if(-not ('Livingroom.Audio.Endpoints' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'windows-audio.cs')}
function Alive($state) {
    $owner=Get-Process -Id $state.ownerPid -ErrorAction SilentlyContinue
    $null -ne $owner -and $owner.StartTime.ToUniversalTime().Ticks.ToString() -eq $state.ownerStarted
}
function Restore($state) {
    # Never touch default devices or volume. Release only the recorded endpoint.
    # If the user already unmuted it, preserve that choice.
    if([Livingroom.Audio.Endpoints]::Muted($state.endpoint)) {
        [Livingroom.Audio.Endpoints]::Mute($state.endpoint,[bool]$state.originalMute)
    }
    Remove-Item -LiteralPath $statePath -ErrorAction Stop
}
if($Action -eq 'Watch') {
    while(Test-Path -LiteralPath $statePath) {
        try {$state=Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json} catch {Start-Sleep -Milliseconds 100;continue}
        if($state.ownerPid -ne $OwnerPid -or $state.session -ne $Session){break}
        if(-not (Alive $state)) {
            # Re-check under the lock in Recover; it cannot release a live owner.
            & $PSCommandPath -Action Recover | Out-Null
            break
        }
        Start-Sleep -Milliseconds 250
    }
    exit
}
$lock=$null
try {
    $deadline=[DateTime]::UtcNow.AddSeconds(2)
    while($null -eq $lock) {
        try {$lock=[IO.File]::Open((Join-Path $root 'local/audio-mute.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)}
        catch [IO.IOException] {if([DateTime]::UtcNow -ge $deadline){throw};Start-Sleep -Milliseconds 50}
    }
    $state=$null
    if(Test-Path -LiteralPath $statePath){$state=Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json}
    if($Action -eq 'Inspect') {
        $endpoint=[Livingroom.Audio.Endpoints]::Default(0)
        [pscustomobject]@{endpoint=$endpoint;muted=[Livingroom.Audio.Endpoints]::Muted($endpoint);volume=[Livingroom.Audio.Endpoints]::Level($endpoint);lease=$state} | ConvertTo-Json -Depth 5 -Compress
    } elseif($Action -eq 'Acquire') {
        if($OwnerPid -le 0){throw 'OwnerPid is required'}
        $owner=Get-Process -Id $OwnerPid -ErrorAction Stop
        if($null -ne $state) {
            if(Alive $state){throw 'Computer mute is owned by a live session'}
            Restore $state
        }
        $endpoint=[Livingroom.Audio.Endpoints]::Default(0)
        $state=[pscustomobject]@{ownerPid=$OwnerPid;ownerStarted=$owner.StartTime.ToUniversalTime().Ticks.ToString();session=$Session;endpoint=$endpoint;originalMute=[Livingroom.Audio.Endpoints]::Muted($endpoint)}
        $state | ConvertTo-Json | Set-Content -LiteralPath ($statePath+'.tmp') -Encoding UTF8
        Move-Item -LiteralPath ($statePath+'.tmp') -Destination $statePath -Force
        try {
            # Separate guardian survives forced termination of the Rust process.
            $watcher=Start-Process -FilePath powershell.exe -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',('"'+$PSCommandPath+'"'),'-Action','Watch','-OwnerPid',$OwnerPid,'-Session',$Session) -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $root "local/mute-watch-$Session.stderr") -RedirectStandardOutput (Join-Path $root "local/mute-watch-$Session.stdout")
            [Livingroom.Audio.Endpoints]::Mute($endpoint,$true)
        } catch {$failure=$_;Restore $state;throw $failure}
        [pscustomobject]@{muted=$true;endpoint=$endpoint;originalMute=$state.originalMute;guardianPid=$watcher.Id} | ConvertTo-Json -Compress
    } elseif($null -ne $state) {
        if($Action -eq 'Release') {
            if($state.ownerPid -ne $OwnerPid -or $state.session -ne $Session){throw 'Computer mute belongs to another session'}
        } elseif(Alive $state){throw 'Cannot recover a live mute session'}
        Restore $state
        '{"restored":true}'
    } else {'{"restored":true}'}
} finally {if($null -ne $lock){$lock.Dispose()}}
