param(
    [string]$Device, [string]$HostIp,
    [ValidateRange(0,86400)][int]$Seconds = 60,
    [ValidateRange(1000,100000)][int]$Bitrate = 20000,
    [ValidateSet('tcp','udp')][string]$Transport='tcp',
    [switch]$TestPattern, [switch]$Build, [switch]$Install
)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$adb=Join-Path $root 'local/tools/platform-tools/adb.exe'
if (!$Device) {
    if (!(Test-Path "$root/config.local.json")) {throw 'Provide -Device with the Fire TV IPv4 address.'}
    $Device=(Get-Content "$root/config.local.json" -Raw | ConvertFrom-Json).device
}
$ip=$Device.Split(':')[0]
$parsedIp=$null
if (![Net.IPAddress]::TryParse($ip,[ref]$parsedIp) -or $parsedIp.AddressFamily -ne [Net.Sockets.AddressFamily]::InterNetwork) {throw 'Invalid IPv4 device address.'}
if (!$Device.Contains(':')) {$Device += ':5555'}
if (!$HostIp) {
    $routeProbe=[Net.Sockets.UdpClient]::new()
    try {$routeProbe.Connect($ip,5004);$HostIp=$routeProbe.Client.LocalEndPoint.Address.ToString()}
    finally {$routeProbe.Dispose()}
}
function Adb([string[]]$arguments) { & $adb @arguments; if($LASTEXITCODE -ne 0) {throw 'ADB command failed.'} }
if ($Build -or !(Test-Path "$root/local/stream-build/streamlab.apk")) { & "$PSScriptRoot/build-client.ps1"; $Install=$true }
Adb @('connect',$Device)
Adb @('-s',$Device,'get-state')
if($Install) {Adb @('-s',$Device,'install','-r',"$root/local/stream-build/streamlab.apk")}
Adb @('-s',$Device,'shell','am','force-stop','dev.livingroom.streamlab')
try {
    Adb @('-s',$Device,'shell','am','start','-n','dev.livingroom.streamlab/.MainActivity','--es','host',$HostIp,'--es','transport',$Transport,'--ei','port','5004')
    Write-Host "1080p60 | $HostIp -> $ip | $Bitrate kbit/s | Ctrl+C to stop"
    $hostArgs=@("$PSScriptRoot/host.py",'--device',$ip,'--transport',$Transport,'--seconds',"$Seconds",'--bitrate',"$Bitrate",'--log',"$root/local/latest-stream.jsonl")
    if($TestPattern) {$hostArgs+='--test-pattern'}
    # Fresh file keeps independent experiments from being merged by the summary tool.
    Set-Content -LiteralPath "$root/local/latest-stream.jsonl" -Value '' -NoNewline
    & python @hostArgs
    if($LASTEXITCODE -ne 0) {throw 'Streaming host failed. See local/latest-stream.jsonl.'}
}
finally { & $adb -s $Device shell am force-stop dev.livingroom.streamlab }
& python "$PSScriptRoot/summarize.py" "$root/local/latest-stream.jsonl"
