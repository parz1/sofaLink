param(
    [ValidateSet('Inspect','Acquire','Release','Recover')][string]$Action='Inspect',
    [int]$OwnerPid=0,
    [string]$Session='manual'
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$statePath=Join-Path $root 'local/audio-route.json'
New-Item -ItemType Directory -Force (Join-Path $root 'local') | Out-Null
if(-not ('Livingroom.Audio.Endpoints' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'windows-audio.cs') }
function Current {
    # Leave the communications role alone, so voice chat keeps its own device.
    @([Livingroom.Audio.Endpoints]::Default(0),[Livingroom.Audio.Endpoints]::Default(1))
}
function SteamEndpoint {
    $matches=@(Get-PnpDevice -Class AudioEndpoint -ErrorAction Stop | Where-Object {
        $_.Status -eq 'OK' -and $_.FriendlyName -match '\(Steam Streaming Speakers\)$' -and $_.InstanceId -match '^SWD\\MMDEVAPI\\\{0\.0\.0\.'
    })
    if($matches.Count -ne 1){throw 'Steam Streaming Speakers is unavailable or ambiguous. Install it through Steam Remote Play; no driver is installed by this tool.'}
    $endpoint=$matches[0].InstanceId.Substring('SWD\MMDEVAPI\'.Length)
    if(-not [Livingroom.Audio.Endpoints]::Active($endpoint)){throw 'Steam audio endpoint is not active'}
    [Livingroom.Audio.Endpoints]::Canonical($endpoint)
}
function OwnerAlive($state) {
    $process=Get-Process -Id $state.ownerPid -ErrorAction SilentlyContinue
    $null -ne $process -and $process.StartTime.ToUniversalTime().Ticks.ToString() -eq $state.ownerStarted
}
function Restore($state) {
    for($role=0;$role -lt 2;$role++) {
        # Compare-and-restore: preserve a user's device change during streaming.
        if([Livingroom.Audio.Endpoints]::Default($role) -eq $state.target -and $state.original[$role] -ne $state.target) {
            [Livingroom.Audio.Endpoints]::Set($state.original[$role],$role)
        }
    }
    Remove-Item -LiteralPath $statePath -ErrorAction Stop
}
$lock=$null
$inspectTarget=if($Action -eq 'Inspect'){SteamEndpoint}else{$null}
try {
    $deadline=[DateTime]::UtcNow.AddSeconds(2)
    while($null -eq $lock) {
        try {$lock=[IO.File]::Open((Join-Path $root 'local/audio-route.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)}
        catch [IO.IOException] {
            if([DateTime]::UtcNow -ge $deadline){throw}
            Start-Sleep -Milliseconds 50
        }
    }
    $state=$null
    if(Test-Path -LiteralPath $statePath){$state=Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json}
    if($Action -eq 'Inspect') {
        [pscustomobject]@{defaults=@(Current);steamEndpoint=$inspectTarget;lease=$state} | ConvertTo-Json -Depth 5 -Compress
    } elseif($Action -eq 'Acquire') {
        if($OwnerPid -le 0 -or [string]::IsNullOrWhiteSpace($Session)){throw 'OwnerPid and Session are required'}
        $owner=Get-Process -Id $OwnerPid -ErrorAction Stop
        if($null -ne $state) {
            if(OwnerAlive $state){throw 'Audio route is owned by a live session'}
            Restore $state
        }
        $target=SteamEndpoint
        $state=[pscustomobject]@{ownerPid=$OwnerPid;ownerStarted=$owner.StartTime.ToUniversalTime().Ticks.ToString();session=$Session;target=$target;original=@(Current)}
        # Save before any mutation: a killed launcher can be recovered later.
        $tempPath=$statePath+'.tmp'
        $state | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $tempPath -Encoding UTF8
        Move-Item -LiteralPath $tempPath -Destination $statePath -Force
        try {
            for($role=0;$role -lt 2;$role++){[Livingroom.Audio.Endpoints]::Set($target,$role)}
        }
        catch { $failure=$_; Restore $state; throw $failure }
        [pscustomobject]@{target=$target;session=$Session;defaults=@(Current)} | ConvertTo-Json -Compress
    } elseif($null -ne $state) {
        if($Action -eq 'Release') {
            if($state.ownerPid -ne $OwnerPid -or $state.session -ne $Session){throw 'Audio route belongs to another session'}
        } elseif(OwnerAlive $state){throw 'Cannot recover a live session'}
        Restore $state
        [pscustomobject]@{restored=$true;defaults=@(Current)} | ConvertTo-Json -Compress
    } else { '{"restored":true,"lease":null}' }
} finally { if($null -ne $lock){$lock.Dispose()} }
