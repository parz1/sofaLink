$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$jdk=(Get-ChildItem "$root/local/tools/jdk" -Directory | Select-Object -First 1).FullName
$out="$root/local/rtp-checks"
New-Item -ItemType Directory -Force $out | Out-Null
& "$jdk/bin/javac.exe" -encoding UTF-8 -d $out "$PSScriptRoot/client/src/dev/livingroom/streamlab/RtpAssembler.java" "$PSScriptRoot/client/tests/RtpAssemblerTest.java"
if($LASTEXITCODE -ne 0){throw 'Test compilation failed'}
& "$jdk/bin/java.exe" -cp $out dev.livingroom.streamlab.RtpAssemblerTest
if($LASTEXITCODE -ne 0){throw 'RTP checks failed'}
& "$jdk/bin/javac.exe" -encoding UTF-8 -d $out "$PSScriptRoot/client/src/dev/livingroom/streamlab/FrameTiming.java" "$PSScriptRoot/client/src/dev/livingroom/streamlab/ClockEstimate.java" "$PSScriptRoot/client/tests/FrameTimingTest.java"
if($LASTEXITCODE -ne 0){throw 'Timing test compilation failed'}
& "$jdk/bin/java.exe" -cp $out dev.livingroom.streamlab.FrameTimingTest
if($LASTEXITCODE -ne 0){throw 'Timing checks failed'}
& "$jdk/bin/javac.exe" -encoding UTF-8 -d $out "$PSScriptRoot/client/src/dev/livingroom/streamlab/AudioClock.java" "$PSScriptRoot/client/tests/AudioClockTest.java"
if($LASTEXITCODE -ne 0){throw 'Audio clock test compilation failed'}
& "$jdk/bin/java.exe" -cp $out dev.livingroom.streamlab.AudioClockTest
if($LASTEXITCODE -ne 0){throw 'Audio clock checks failed'}
& "$jdk/bin/javac.exe" -encoding UTF-8 -d $out "$PSScriptRoot/client/src/dev/livingroom/streamlab/PresentationClock.java" "$PSScriptRoot/client/tests/PresentationClockTest.java"
if($LASTEXITCODE -ne 0){throw 'Presentation test compilation failed'}
& "$jdk/bin/java.exe" -cp $out dev.livingroom.streamlab.PresentationClockTest
if($LASTEXITCODE -ne 0){throw 'Presentation clock checks failed'}
