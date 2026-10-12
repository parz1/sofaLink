param([ValidateSet('healthy','startup-fail','startup-timeout','audio-stall','video-stall','disconnect','stop-fail')][string]$Scenario='healthy', [switch]$Emulator, [switch]$FireTv, [string]$Device, [string]$StreamRequest, [switch]$Simulate, [switch]$TestSource, [switch]$SteamBigPicture, [switch]$Diagnostics, [switch]$VirtualAudio, [switch]$KeepHostAudio)
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
$previousInputWindow=$env:LIVINGROOM_INPUT_WINDOW
$previousVirtualAudio=$env:LIVINGROOM_VIRTUAL_AUDIO
$previousHostMute=$env:LIVINGROOM_HOST_MUTE
try {
    $env:LIVINGROOM_VIRTUAL_AUDIO='0'
    $env:LIVINGROOM_HOST_MUTE=if($KeepHostAudio){'0'}else{'1'}
    if($VirtualAudio) {
        if($Simulate -or $TestSource -or $PSBoundParameters.ContainsKey('Scenario')){throw '-VirtualAudio requires actual desktop streaming'}
        $env:LIVINGROOM_VIRTUAL_AUDIO='1'
    }
    if($SteamBigPicture) {
        if($Simulate -or $TestSource -or $PSBoundParameters.ContainsKey('Scenario')){throw '-SteamBigPicture requires actual desktop streaming'}
        $steamExe=(Get-ItemProperty 'HKCU:\Software\Valve\Steam' -ErrorAction Stop).SteamExe
        # Steam is the interactive target explicitly selected by the user.
        Start-Process -FilePath $steamExe -ArgumentList 'steam://open/bigpicture' -WindowStyle Normal
        $steamDeadline=[DateTime]::UtcNow.AddSeconds(25)
        do {
            $steamWindow=Get-Process steamwebhelper -ErrorAction SilentlyContinue | Where-Object {$_.MainWindowTitle -match '大屏幕|Big Picture'} | Select-Object -First 1
            if($steamWindow){break}
            Start-Sleep -Milliseconds 250
        }while([DateTime]::UtcNow -lt $steamDeadline)
        if(!$steamWindow){throw 'Steam Big Picture window not found'}
        $env:LIVINGROOM_INPUT_WINDOW=$steamWindow.MainWindowHandle.ToInt64().ToString()
    }
    if (($Emulator -and $Simulate) -or ($FireTv -and ($Emulator -or $Simulate -or $PSBoundParameters.ContainsKey('Scenario')))) {throw 'Choose -FireTv, -Emulator or -Simulate'}
    if ($Device -and !$FireTv) {throw '-Device requires -FireTv'}
    if ($StreamRequest -and !$FireTv) {throw '-StreamRequest currently requires the real device adapter (-FireTv)'}
    if ($FireTv) {
        if (!$Device) {$Device=(Get-Content "$PSScriptRoot/config.local.json" -Raw | ConvertFrom-Json).device}
        $ip=$Device -replace ':5555$',''
        $parsed=$null
        if (![Net.IPAddress]::TryParse($ip,[ref]$parsed) -or $parsed.AddressFamily -ne [Net.Sockets.AddressFamily]::InterNetwork) {throw 'Fire TV requires an IPv4 address'}
        $adb=Join-Path $env:LOCALAPPDATA 'Android/Sdk/platform-tools/adb.exe'
        & $adb connect ($ip+':5555')
        if($LASTEXITCODE -ne 0){throw 'Fire TV ADB connection failed'}
        $tuiArgs=@('--fire-tv',$ip)
        if ($TestSource) {$tuiArgs+='--test-source'}
        if ($Diagnostics) {$tuiArgs+='--diagnostics'}
    } elseif ($Emulator -or (!$Simulate -and !$PSBoundParameters.ContainsKey('Scenario'))) {
        & "$PSScriptRoot/scripts/tv-emulator.ps1" -Action Start
        $tuiArgs=@('--emulator')
        if ($TestSource) { $tuiArgs+='--test-source' }
        if ($Diagnostics) {$tuiArgs+='--diagnostics'}
    } else {
        if ($TestSource -or $Diagnostics) { throw '-TestSource / -Diagnostics requires -Emulator' }
        $tuiArgs=@('--scenario',$Scenario)
    }
    if ($StreamRequest) {$tuiArgs+=@('--stream-request',(Resolve-Path -LiteralPath $StreamRequest).Path)}
    & cargo run --locked -p sofalink-tui -- @tuiArgs
    if ($LASTEXITCODE -ne 0) { throw '会话退出时报告了异常，请查看终端说明和会话日志。' }
}
finally { $env:LIVINGROOM_INPUT_WINDOW=$previousInputWindow; $env:LIVINGROOM_VIRTUAL_AUDIO=$previousVirtualAudio; $env:LIVINGROOM_HOST_MUTE=$previousHostMute; Pop-Location }
