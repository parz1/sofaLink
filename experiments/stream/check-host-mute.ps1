# Same application, capture pipeline and level; only endpoint mute changes.
param([ValidateSet('process','endpoint','both')][string]$Capture='process')
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if(Test-Path (Join-Path $root 'local/audio-route.json')){throw 'End the virtual audio session first'}
if(@(Get-NetTCPConnection -LocalPort 5004,5006 -State Listen -ErrorAction SilentlyContinue).Count){throw 'End the active stream first'}
Add-Type -Path (Join-Path $root 'scripts/windows-audio.cs')
$endpoint=[Livingroom.Audio.Endpoints]::Default(0)
$defaults=@(0,1,2 | ForEach-Object {[Livingroom.Audio.Endpoints]::Default($_)})
$muted=[Livingroom.Audio.Endpoints]::Muted($endpoint)
$level=[Livingroom.Audio.Endpoints]::Level($endpoint)
$out=Join-Path $root 'local/host-mute-check'
New-Item -ItemType Directory -Force $out | Out-Null
$wav=Join-Path $out 'tone.wav'
$writer=[IO.BinaryWriter]::new([IO.File]::Create($wav))
try {
    $writer.Write([Text.Encoding]::ASCII.GetBytes('RIFF'));$writer.Write([int]96036)
    $writer.Write([Text.Encoding]::ASCII.GetBytes('WAVEfmt '));$writer.Write([int]16)
    $writer.Write([int16]1);$writer.Write([int16]1);$writer.Write([int]48000);$writer.Write([int]96000)
    $writer.Write([int16]2);$writer.Write([int16]16);$writer.Write([Text.Encoding]::ASCII.GetBytes('data'));$writer.Write([int]96000)
    for($i=0;$i -lt 48000;$i++){$writer.Write([int16](400*[Math]::Sin(2*[Math]::PI*440*$i/48000)))}
} finally {$writer.Dispose()}
$helper=Join-Path $out 'capture.py'
@'
import sys,time,array,math,json
sys.path.insert(0,sys.argv[1]);import host
Gst=host.Gst;Gst.init(None)
source='wasapi2src name=source loopback=true low-latency=true provide-clock=false loopback-silence-on-device-mute=false'
if sys.argv[3]=='process': source+=' loopback-mode=include-process-tree loopback-target-pid='+sys.argv[4]
p=Gst.parse_launch(source+' ! audioconvert ! audioresample ! audio/x-raw,format=S16LE,rate=48000,channels=2 ! appsink name=sink sync=false max-buffers=2 drop=true')
if sys.argv[3]=='endpoint':p.get_by_name('source').set_property('device',sys.argv[2])
samples=0;energy=0;active=0;peak=0;sink=p.get_by_name('sink')
try:
 p.set_state(Gst.State.PLAYING);until=time.monotonic()+3
 while time.monotonic()<until:
  error=p.get_bus().pop_filtered(Gst.MessageType.ERROR)
  if error is not None:raise RuntimeError(str(error.parse_error()))
  sample=sink.emit('try-pull-sample',200*Gst.MSECOND)
  if sample is None:continue
  b=sample.get_buffer();a=array.array('h',b.extract_dup(0,b.get_size()))
  if a:
   e=sum(v*v for v in a);samples+=len(a);energy+=e
   rms=math.sqrt(e/len(a))/32768;peak=max(peak,rms)
   if rms>0.001:active+=len(a)
finally:p.set_state(Gst.State.NULL)
print(json.dumps({'samples':samples,'active_samples':active,'rms':math.sqrt(energy/max(1,samples))/32768,'peak_rms':peak,'mode':sys.argv[3]}))
if active<48000 or peak<0.001:raise SystemExit('Test tone was not captured')
'@ | Set-Content -LiteralPath $helper -Encoding UTF8
$python=(Get-Content (Join-Path $root 'local/emulator-runtime.json') -Raw | ConvertFrom-Json).python
$player=$null;$rows=@()
try {
    $player=[System.Media.SoundPlayer]::new($wav);$player.Load()
    $modes=if($Capture -eq 'both'){@('process','endpoint')}else{@($Capture)}
    foreach($mode in $modes) {
        foreach($mute in @($false,$true)) {
            [Livingroom.Audio.Endpoints]::Mute($endpoint,$mute)
            $player.PlayLooping()
            $text=& $python $helper $PSScriptRoot $endpoint $mode $PID
            if($LASTEXITCODE -ne 0){throw "Capture failed: $mode muted=$mute"}
            $row=$text | ConvertFrom-Json;$row | Add-Member muted $mute;$rows+=$row
            $player.Stop()
        }
    }
    foreach($mode in $modes) {
        $pair=@($rows | Where-Object mode -eq $mode)
        $ratio=$pair[1].peak_rms/$pair[0].peak_rms
        if($ratio -lt 0.8 -or $ratio -gt 1.2){throw "Captured tone level changed with mute: $mode ratio=$ratio"}
    }
} finally {
    if($null -ne $player){$player.Stop();$player.Dispose()}
    [Livingroom.Audio.Endpoints]::Mute($endpoint,$muted)
    if([Livingroom.Audio.Endpoints]::Level($endpoint) -ne $level){throw 'Endpoint volume changed unexpectedly'}
    if((@(0,1,2 | ForEach-Object {[Livingroom.Audio.Endpoints]::Default($_)}) -join '|') -ne ($defaults -join '|')){throw 'Default audio devices changed unexpectedly'}
    [pscustomobject]@{endpoint=$endpoint;originalMute=$muted;volume=$level;rows=$rows;restored=[Livingroom.Audio.Endpoints]::Muted($endpoint) -eq $muted} | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $out ('result-'+$Capture+'.json')) -Encoding UTF8
}
Write-Output "PASS: endpoint mute preserves $Capture loopback tone; defaults, volume and original mute restored"
