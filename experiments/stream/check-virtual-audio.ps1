# Synthetic local-application audio -> Steam virtual endpoint -> WASAPI capture.
param([string]$Python='', [ValidateSet('process','endpoint')][string]$Capture='process')
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$route=Join-Path $root 'scripts/audio-route.ps1'
if(Test-Path (Join-Path $root 'local/audio-route.json')){throw 'End the current audio session first'}
if(@(Get-NetTCPConnection -LocalPort 5004,5006 -State Listen -ErrorAction SilentlyContinue).Count){throw 'End the active stream first'}
if(-not $Python){$Python=(Get-Content (Join-Path $root 'local/emulator-runtime.json') -Raw | ConvertFrom-Json).python}
$out=Join-Path $root 'local/virtual-audio-check'
New-Item -ItemType Directory -Force $out | Out-Null
$wav=Join-Path $out 'tone.wav'
$writer=[IO.BinaryWriter]::new([IO.File]::Create($wav))
try {
    $writer.Write([Text.Encoding]::ASCII.GetBytes('RIFF'));$writer.Write([int]96036)
    $writer.Write([Text.Encoding]::ASCII.GetBytes('WAVEfmt '));$writer.Write([int]16)
    $writer.Write([int16]1);$writer.Write([int16]1);$writer.Write([int]48000);$writer.Write([int]96000)
    $writer.Write([int16]2);$writer.Write([int16]16)
    $writer.Write([Text.Encoding]::ASCII.GetBytes('data'));$writer.Write([int]96000)
    for($i=0;$i -lt 48000;$i++){$writer.Write([int16](600*[Math]::Sin(2*[Math]::PI*440*$i/48000)))}
} finally {$writer.Dispose()}
$helper=Join-Path $out 'capture.py'
@'
import sys, time, array, math, json
sys.path.insert(0, sys.argv[1])
import host
Gst=host.Gst
Gst.init(None)
source='wasapi2src name=source loopback=true low-latency=true provide-clock=false'
if sys.argv[3]=='process': source+=' loopback-mode=include-process-tree loopback-target-pid='+sys.argv[4]
p=Gst.parse_launch(source+' ! audioconvert ! audioresample ! audio/x-raw,format=S16LE,rate=48000,channels=2 ! appsink name=sink sync=false max-buffers=2 drop=true')
if sys.argv[3]=='endpoint': p.get_by_name('source').set_property('device',sys.argv[2])
sink=p.get_by_name('sink'); samples=0; peak=0.0
try:
    p.set_state(Gst.State.PLAYING)
    until=time.monotonic()+4
    while time.monotonic()<until:
        error=p.get_bus().pop_filtered(Gst.MessageType.ERROR)
        if error is not None: raise RuntimeError(str(error.parse_error()))
        sample=sink.emit('try-pull-sample',200*Gst.MSECOND)
        if sample is None: continue
        b=sample.get_buffer(); a=array.array('h',b.extract_dup(0,b.get_size()))
        samples+=len(a)
        if a: peak=max(peak,math.sqrt(sum(v*v for v in a)/len(a))/32768)
finally: p.set_state(Gst.State.NULL)
print(json.dumps({'samples':samples,'peak_rms':peak,'endpoint':sys.argv[2],'capture':sys.argv[3]}))
if samples==0 or peak<0.003: raise SystemExit('Virtual endpoint capture did not receive the test tone')
'@ | Set-Content -LiteralPath $helper -Encoding UTF8
$session='virtual-capture-'+[Guid]::NewGuid().ToString('N')
$player=$null
try {
    $lease=& $route -Action Acquire -OwnerPid $PID -Session $session | ConvertFrom-Json
    $player=[System.Media.SoundPlayer]::new($wav);$player.Load();$player.PlayLooping()
    & $Python $helper $PSScriptRoot $lease.target $Capture $PID | Tee-Object -FilePath (Join-Path $out ('result-'+$Capture+'.json'))
    if($LASTEXITCODE -ne 0){throw 'Virtual audio capture failed'}
} finally {
    if($null -ne $player){$player.Stop();$player.Dispose()}
    & $route -Action Release -OwnerPid $PID -Session $session | Out-Null
}
