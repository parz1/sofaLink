param(
    [ValidateSet('sdr709','goldfish-compat')][string]$Profile='goldfish-compat',
    [string]$Device='',
    [ValidateRange(1,12)][int]$Captures=1,
    [ValidateSet('p1','p4')][string]$Preset='p4',
    [ValidateSet('baseline','high')][string]$EncoderProfile='',
    [ValidateRange(0,600)][int]$GopSize=0,
    [ValidateRange(0,8)][int]$VbvFrames=0,
    [switch]$DesktopCapture,
    [ValidateSet('srgb','bt709')][string]$Transfer='',
    [ValidateRange(0,90)][int]$HoldSeconds=0,
    [switch]$TextureVideo,
    [string]$Sdk=(Join-Path $env:LOCALAPPDATA 'Android/Sdk')
)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$adb=Join-Path $Sdk 'platform-tools/adb.exe'
$serial=if($Device){$Device+':5555'}else{'emulator-5580'}
if(!$EncoderProfile){$EncoderProfile=if($Device){'high'}else{'baseline'}}
if(!$GopSize){$GopSize=if($Device){120}else{15}}
if(!$VbvFrames){$VbvFrames=if($Device){4}else{1}}
$vbvKbit=[int][Math]::Floor(20000*$VbvFrames/60)
$hostAddress='10.0.2.2'
function Adb([string[]]$Arguments) {
    & $adb -s $serial @Arguments
    if($LASTEXITCODE -ne 0){throw 'Color check ADB operation failed'}
}
if($Device){
    $ip=[Net.IPAddress]::Parse($Device)
    if($ip.AddressFamily -ne [Net.Sockets.AddressFamily]::InterNetwork){throw 'Device must be an IPv4 address'}
    if($Profile -ne 'sdr709'){throw 'Fire TV requires the sdr709 profile'}
    $model=Adb @('shell','getprop','ro.product.model')
    if($model -notmatch '^AFT'){throw 'Expected Fire TV device'}
    $route=[Net.Sockets.UdpClient]::new()
    try{$route.Connect($Device,5014);$hostAddress=$route.Client.LocalEndPoint.Address.ToString()}finally{$route.Dispose()}
}else{
    $name=Adb @('emu','avd','name')
    if($name -notcontains 'Livingroom_TV_API_36'){throw 'Color check requires the dedicated Livingroom TV emulator'}
}
if(@(Get-NetTCPConnection -LocalPort 5004,5006,5014 -State Listen -ErrorAction SilentlyContinue).Count){throw 'End the active stream/color check before running the color check'}
$out=Join-Path $root ('local/color-checks/'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $out | Out-Null
Add-Type -AssemblyName System.Drawing
$bitmap=[Drawing.Bitmap]::new(1920,1080)
$graphics=[Drawing.Graphics]::FromImage($bitmap)
$graphics.Clear([Drawing.Color]::FromArgb(24,24,24))
$colors=@([Drawing.Color]::White,[Drawing.Color]::Yellow,[Drawing.Color]::Cyan,[Drawing.Color]::Lime,[Drawing.Color]::Magenta,[Drawing.Color]::Red,[Drawing.Color]::Blue,[Drawing.Color]::Black)
for($i=0;$i -lt 8;$i++) {
    $brush=[Drawing.SolidBrush]::new($colors[$i])
    $graphics.FillRectangle($brush,$i*240,160,240,320);$brush.Dispose()
}
for($i=0;$i -lt 16;$i++) {
    $gray=$i*17;$brush=[Drawing.SolidBrush]::new([Drawing.Color]::FromArgb($gray,$gray,$gray))
    $graphics.FillRectangle($brush,$i*120,520,120,180);$brush.Dispose()
}
for($i=0;$i -lt 480;$i++) {
    $graphics.FillRectangle([Drawing.Brushes]::White,240+$i*3,740,1,200)
    $graphics.FillRectangle([Drawing.Brushes]::Red,241+$i*3,740,1,200)
    $graphics.FillRectangle([Drawing.Brushes]::Blue,242+$i*3,740,1,200)
}
$font=[Drawing.Font]::new('Consolas',32)
$graphics.DrawString('RGB COLOR CHECK / 1920x1080 / SDR',$font,[Drawing.Brushes]::White,40,35)
$bitmap.Save("$out/reference.png",[Drawing.Imaging.ImageFormat]::Png)
$font.Dispose();$graphics.Dispose();$bitmap.Dispose()
# Bootstrap the existing bundled native GStreamer CLI through the host's runtime.
# Media stays in GStreamer / D3D11 / NVENC; this temporary helper runs no frame logic.
@'
import sys, subprocess
from pathlib import Path
sys.path.insert(0,str(Path(sys.argv[1])/'experiments/stream'))
import host
raise SystemExit(subprocess.run([str(host.BUNDLE/'gstreamer_cli/bin/gst-launch-1.0.exe'),*sys.argv[2:]]).returncode)
'@ | Set-Content -LiteralPath "$out/run-gst.py" -Encoding utf8
$runtime=Get-Content "$root/local/emulator-runtime.json" -Raw | ConvertFrom-Json
$colorimetry=if($Profile -eq 'goldfish-compat'){'2:4:7:1'}else{'2:3:7:1'}
if(!$Transfer){$Transfer=if($Profile -eq 'goldfish-compat'){'srgb'}else{'bt709'}}
if($Transfer -eq 'bt709'){$colorimetry=if($Profile -eq 'goldfish-compat'){'2:4:5:1'}else{'2:3:5:1'}}
$path=(Join-Path $out 'reference.png').Replace('\','/')
$source=if($DesktopCapture){@('d3d11screencapturesrc','show-cursor=false','!','videorate','drop-only=true','!','video/x-raw(memory:D3D11Memory),framerate=60/1')}else{
    @('filesrc',('location="'+$path+'"'),'!','pngdec','!','imagefreeze','is-live=true',('num-buffers='+[Math]::Max(3600,($HoldSeconds+40)*60)),'!','video/x-raw,framerate=60/1','!','d3d11upload')
}
$pipeline=$source+@('!','d3d11convert',
    '!',"video/x-raw(memory:D3D11Memory),format=NV12,width=1920,height=1080,colorimetry=$colorimetry",
    '!','nvd3d11h264enc',"preset=$Preset",'tune=ultra-low-latency','rc-mode=cbr','bitrate=20000',"vbv-buffer-size=$vbvKbit","gop-size=$GopSize",'bframes=0','rc-lookahead=0','zerolatency=true',
    '!',"video/x-h264,stream-format=byte-stream,alignment=au,profile=$EncoderProfile",
    '!','tee','name=encoded','!','queue','!','filesink',('location="'+($out.Replace('\','/')+'/encoded.h264')+'"'),'sync=false','encoded.','!','queue',
    '!','rtph264pay','pt=96','mtu=1200','config-interval=-1','aggregate-mode=none','timestamp-offset=0',
    '!','rtpstreampay','!','tcpserversink','host=0.0.0.0','port=5014','sync=true','async=false')
$process=[Diagnostics.Process]::new()
$process.StartInfo.FileName=$runtime.python
$process.StartInfo.UseShellExecute=$false;$process.StartInfo.CreateNoWindow=$true
$process.StartInfo.RedirectStandardOutput=$true;$process.StartInfo.RedirectStandardError=$true
foreach($arg in (@("$out/run-gst.py",$root)+$pipeline)){$process.StartInfo.ArgumentList.Add($arg)}
$stdout=$null;$stderr=$null;$owned=$false;$started=$false;$chart=$null
try {
    if($DesktopCapture){
@'
param([string]$Image)
Add-Type -AssemblyName System.Windows.Forms
$form=[Windows.Forms.Form]::new()
$form.FormBorderStyle='None';$form.Bounds=[Windows.Forms.Screen]::PrimaryScreen.Bounds
$form.TopMost=$true;$form.BackgroundImage=[Drawing.Image]::FromFile($Image);$form.BackgroundImageLayout='Stretch'
[Windows.Forms.Application]::Run($form)
'@ | Set-Content -LiteralPath "$out/show-chart.ps1" -Encoding utf8
        $chart=[Diagnostics.Process]::new()
        $chart.StartInfo.FileName=(Get-Process -Id $PID).Path
        $chart.StartInfo.UseShellExecute=$false;$chart.StartInfo.CreateNoWindow=$true
        foreach($arg in @('-NoProfile','-File',"$out/show-chart.ps1","$out/reference.png")){$chart.StartInfo.ArgumentList.Add($arg)}
        if(!$chart.Start()){throw 'Reference desktop launch failed'}
        Start-Sleep -Seconds 2
    }
    Adb @('shell','am','force-stop','dev.livingroom.streamlab') | Out-Null
    $owned=$true
    $decoderArgs=if($Device){@('--es','deviceMode','fire-tv')}else{@('--es','decoder','c2.goldfish.h264.decoder')}
    if(!$process.Start()){throw 'Color source launch failed'}
    $started=$true
    $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
    Start-Sleep -Seconds 2
    $session='color-'+(Split-Path $out -Leaf)
    Adb (@('shell','am','start','-n','dev.livingroom.streamlab/.MainActivity','--es','host',$hostAddress,'--es','sessionId',$session,'--es','transport','tcp')+$decoderArgs+@('--ei','port','5014','--ez','audio','false','--ez','textureVideo',$TextureVideo.IsPresent.ToString().ToLowerInvariant())) | Out-Null
    $deadline=[DateTime]::UtcNow.AddSeconds(25);$ready=$false
    while([DateTime]::UtcNow -lt $deadline){
        $lines=Adb @('logcat','-d','-t','80','-s','StreamLab:I')
        foreach($line in $lines){
            if($line -match 'METRICS (.*)$'){
                $metric=$Matches[1] | ConvertFrom-Json
                if($metric.sessionId -eq $session -and $metric.videoHealthy -and $metric.rendered -ge 120){$ready=$true}
            }
        }
        if($ready){break}
        if($process.HasExited){throw 'Color source exited while waiting for video'}
        Start-Sleep -Seconds 1
    }
    if(!$ready){throw 'Client did not confirm sustained video playback'}
    if($process.HasExited){throw 'Color source exited before capture'}
    Adb @('shell','input','keyevent','82') | Out-Null
    Adb @('shell','screencap','-p','/sdcard/livingroom-color-check.png') | Out-Null
    Adb @('pull','/sdcard/livingroom-color-check.png',"$out/client.png") | Out-Null
    for($frame=1;$frame -lt $Captures;$frame++){
        Adb @('shell','screencap','-p','/sdcard/livingroom-color-check.png') | Out-Null
        Adb @('pull','/sdcard/livingroom-color-check.png',"$out/client-$frame.png") | Out-Null
    }
    $client=[Drawing.Bitmap]::new("$out/client.png")
    try {
        if($client.Width -ne 1920 -or $client.Height -ne 1080){throw 'Unexpected client screenshot size'}
        $samples=@();$maximum=0
        for($i=0;$i -lt 24;$i++) {
            $expected=if($i -lt 8){$colors[$i]}else{$v=($i-8)*17;[Drawing.Color]::FromArgb($v,$v,$v)}
            $x=if($i -lt 8){120+$i*240}else{60+($i-8)*120}
            $y=if($i -lt 8){300}else{600}
            $actual=$client.GetPixel($x,$y)
            $target=@([int]$expected.R,[int]$expected.G,[int]$expected.B)
            $value=@([int]$actual.R,[int]$actual.G,[int]$actual.B)
            for($c=0;$c -lt 3;$c++){$maximum=[Math]::Max($maximum,[Math]::Abs($value[$c]-$target[$c]))}
            $samples+=@{patch=$i;expected=$target;actual=$value}
        }
        @{profile=$Profile;preset=$Preset;encoder_profile=$EncoderProfile;gop_size=$GopSize;vbv_frames=$VbvFrames;texture_video=$TextureVideo.IsPresent;desktop_capture=$DesktopCapture.IsPresent;device=$serial;colorimetry=$colorimetry;maximum_channel_error=$maximum;samples=$samples;scope='24 static RGB/gray patch centers; not a full-frame or physical panel measurement'} |
            ConvertTo-Json -Depth 6 | Set-Content -LiteralPath "$out/result.json" -Encoding utf8
    } finally {$client.Dispose()}
    Write-Output "Color check: $Profile / maximum RGB channel error $maximum / $out"
    Adb @('logcat','-d','-t','250','-s','StreamLab:I') | Set-Content -LiteralPath "$out/client.log" -Encoding utf8
    if($HoldSeconds){Start-Sleep -Seconds $HoldSeconds}
    if($maximum -gt 3){throw 'Color check failed: static patches differ by more than 3 / 255'}
} finally {
    if($started -and !$process.HasExited){$process.Kill($true);$process.WaitForExit()}
    if($stdout){$stdout.GetAwaiter().GetResult() | Set-Content -LiteralPath "$out/pipeline.stdout" -Encoding utf8}
    if($stderr){$stderr.GetAwaiter().GetResult() | Set-Content -LiteralPath "$out/pipeline.stderr" -Encoding utf8}
    $process.Dispose()
    if($chart){if(!$chart.HasExited){$chart.Kill();$chart.WaitForExit()};$chart.Dispose()}
    if($owned){Adb @('shell','am','force-stop','dev.livingroom.streamlab') | Out-Null}
}
