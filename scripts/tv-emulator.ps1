param(
    [ValidateSet('Start','Status','Install','Stop')][string]$Action = 'Start',
    [string]$Sdk = (Join-Path $env:LOCALAPPDATA 'Android/Sdk')
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$adb = Join-Path $Sdk 'platform-tools/adb.exe'
$emulator = Join-Path $Sdk 'emulator/emulator.exe'
$serial = 'emulator-5580'
$avd = 'Livingroom_TV_API_36'
if (!(Test-Path -LiteralPath $adb)) { throw "ADB not found in $Sdk" }
function Invoke-Adb([string[]]$Arguments) {
    & $adb -s $serial @Arguments
    if ($LASTEXITCODE -ne 0) { throw "ADB failed: $($Arguments -join ' ')" }
}
function Get-DeviceState {
    $devices = & $adb devices
    if ($LASTEXITCODE -ne 0) { throw 'Unable to list devices' }
    $entry = $devices | Where-Object { $_ -match "^$serial\s+" } | Select-Object -First 1
    if ($entry) { return ($entry -split '\s+')[1] }
    return 'absent'
}
function Confirm-Tv {
    $name = Invoke-Adb @('emu','avd','name')
    if ($name -notcontains $avd) { throw "$serial is not $avd; refusing to operate on it" }
}
function Save-Runtime {
    $runtimeFile=Join-Path $repoRoot 'local/emulator-runtime.json'
    $runtimeConfig=if (Test-Path -LiteralPath $runtimeFile) {Get-Content -LiteralPath $runtimeFile -Raw | ConvertFrom-Json -AsHashtable} else {@{python=(Get-Command python.exe -ErrorAction Stop).Source}}
    $tvProcess=@(Get-CimInstance Win32_Process -Filter "name='qemu-system-x86_64.exe'" | Where-Object { $_.CommandLine -match 'Livingroom_TV_API_36' })
    if ($tvProcess.Count -ne 1) {throw 'Unable to identify the TV emulator audio process'}
    $runtimeConfig.emulatorPid=$tvProcess[0].ProcessId
    $runtimeConfig | ConvertTo-Json | Set-Content -LiteralPath $runtimeFile -Encoding utf8
}
switch ($Action) {
    'Start' {
        if (!(Test-Path -LiteralPath $emulator)) { throw "Emulator not found in $Sdk" }
        if ((& $emulator -list-avds) -notcontains $avd) { throw "Create $avd first; see docs/android-tv-emulator.md" }
        $deviceState=Get-DeviceState
        $tvRunning=@(Get-CimInstance Win32_Process -Filter "name='qemu-system-x86_64.exe'" | Where-Object { $_.CommandLine -match 'Livingroom_TV_API_36' })
        if ($deviceState -eq 'absent' -or ($deviceState -eq 'offline' -and $tvRunning.Count -eq 0)) {
            New-Item -ItemType Directory -Path "$repoRoot/local" -Force | Out-Null
            # This is the interactive TV screen the user needs to see and control.
            Start-Process -FilePath $emulator -ArgumentList @('-avd',$avd,'-port','5580','-gpu','auto') -WindowStyle Normal -RedirectStandardOutput "$repoRoot/local/tv-emulator.log" -RedirectStandardError "$repoRoot/local/tv-emulator-error.log" | Out-Null
        }
        $deadline = [DateTime]::UtcNow.AddSeconds(150)
        do {
            if ((Get-DeviceState) -eq 'device') {
                $booted = & $adb -s $serial shell getprop sys.boot_completed
                if ($LASTEXITCODE -eq 0 -and "$booted".Trim() -eq '1') {
                    Confirm-Tv
                    Save-Runtime
                    Write-Output "Ready: $avd ($serial)"
                    return
                }
            }
            Start-Sleep -Seconds 2
        } while ([DateTime]::UtcNow -lt $deadline)
        throw 'TV emulator boot timed out; inspect local/tv-emulator*.log'
    }
    'Status' {
        $state = Get-DeviceState
        Write-Output "$serial : $state"
        if ($state -eq 'device') {
            Confirm-Tv
            Invoke-Adb @('shell','getprop','sys.boot_completed')
            Invoke-Adb @('shell','wm','size')
        }
    }
    'Install' {
        Confirm-Tv
        Save-Runtime
        $apk = Join-Path $repoRoot 'local/stream-build/streamlab.apk'
        if (!(Test-Path -LiteralPath $apk)) { throw 'Build first: ./experiments/stream/build-client.ps1' }
        Invoke-Adb @('install','-r',$apk)
        Invoke-Adb @('shell','am','start','-W','-n','dev.livingroom.streamlab/.MainActivity','--es','host','10.0.2.2','--es','transport','tcp','--es','decoder','c2.goldfish.h264.decoder','--ez','syncVideo','true','--ei','videoQueueFrames','16')
    }
    'Stop' {
        if ((Get-DeviceState) -eq 'absent') { Write-Output 'TV emulator already stopped'; return }
        Confirm-Tv
        Invoke-Adb @('emu','kill')
        $deadline=[DateTime]::UtcNow.AddSeconds(45)
        do {
            $tvRunning=@(Get-CimInstance Win32_Process -Filter "name='qemu-system-x86_64.exe'" | Where-Object { $_.CommandLine -match 'Livingroom_TV_API_36' })
            if ($tvRunning.Count -eq 0 -and (Get-DeviceState) -eq 'absent') {Write-Output 'TV emulator stopped';return}
            Start-Sleep -Seconds 1
        } while ([DateTime]::UtcNow -lt $deadline)
        throw 'TV emulator shutdown timed out; wait for snapshot saving before restarting'
    }
}
