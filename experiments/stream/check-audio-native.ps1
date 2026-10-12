param([string]$Serial='emulator-5580',[string]$Sdk=(Join-Path $env:LOCALAPPDATA 'Android/Sdk'))
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$ndk=(Get-ChildItem "$root/local/tools/ndk" -Directory | Select-Object -First 1).FullName
$adb=Join-Path $Sdk 'platform-tools/adb.exe'
$name=& $adb -s $Serial emu avd name
if($LASTEXITCODE -ne 0 -or $name -notcontains 'Livingroom_TV_API_36'){throw 'Native tests require the dedicated Livingroom TV emulator'}
$out=Join-Path $root 'local/native-audio-checks'
New-Item -ItemType Directory -Force $out | Out-Null
& "$ndk/toolchains/llvm/prebuilt/windows-x86_64/bin/clang.exe" --target=x86_64-linux-android30 -O2 -Wall -Wextra "$PSScriptRoot/client/native/audio_ring_test.c" -o "$out/audio-ring-test"
if($LASTEXITCODE -ne 0){throw 'Native audio test compilation failed'}
& $adb -s $Serial push "$out/audio-ring-test" /data/local/tmp/livingroom-audio-ring-test
if($LASTEXITCODE -ne 0){throw 'Native test upload failed'}
& $adb -s $Serial shell chmod 700 /data/local/tmp/livingroom-audio-ring-test
if($LASTEXITCODE -ne 0){throw 'Native test permissions failed'}
& $adb -s $Serial shell /data/local/tmp/livingroom-audio-ring-test
if($LASTEXITCODE -ne 0){throw 'Native audio checks failed'}
