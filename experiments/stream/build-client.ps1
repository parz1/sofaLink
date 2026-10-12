param([string]$Jdk, [string]$AndroidJar, [string]$BuildTools, [string]$Ndk)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (!$Jdk) { $Jdk = (Get-ChildItem "$root/local/tools/jdk" -Directory | Select-Object -First 1).FullName }
if (!$AndroidJar) { $AndroidJar = (Get-ChildItem "$root/local/tools/android-platform" -Recurse -Filter android.jar | Select-Object -First 1).FullName }
if (!$BuildTools) { $BuildTools = (Get-ChildItem "$root/local/tools/android-build" -Recurse -Filter aapt2.exe | Select-Object -First 1).DirectoryName }
$out = Join-Path $root 'local/stream-build'
New-Item -ItemType Directory -Force -Path "$out/classes","$out/dex" | Out-Null
function Invoke-Checked([string]$exe, [string[]]$arguments) { & $exe @arguments; if ($LASTEXITCODE -ne 0) { throw "Failed: $exe" } }
$sources = @(Get-ChildItem "$PSScriptRoot/client/src" -Recurse -Filter '*.java' | ForEach-Object FullName)
Invoke-Checked "$Jdk/bin/javac.exe" (@('-encoding','UTF-8','--release','8','-classpath',$AndroidJar,'-d',"$out/classes") + $sources)
Invoke-Checked "$Jdk/bin/jar.exe" @('cf',"$out/classes.jar",'-C',"$out/classes",'.')
# Run d8/apksigner Java jars directly; no global JAVA_HOME or Android SDK install required.
Invoke-Checked "$Jdk/bin/java.exe" @('-cp',"$BuildTools/lib/d8.jar",'com.android.tools.r8.D8','--min-api','30','--lib',$AndroidJar,'--output',"$out/dex","$out/classes.jar")
Invoke-Checked "$BuildTools/aapt2.exe" @('link','-o',"$out/unsigned.apk",'-I',$AndroidJar,'--manifest',"$PSScriptRoot/client/AndroidManifest.xml")
Invoke-Checked "$Jdk/bin/jar.exe" @('uf',"$out/unsigned.apk",'-C',"$out/dex",'classes.dex')
if (!$Ndk -and (Test-Path "$root/local/tools/ndk")) { $Ndk=(Get-ChildItem "$root/local/tools/ndk" -Directory | Select-Object -First 1).FullName }
if ($Ndk) {
    $llvm="$Ndk/toolchains/llvm/prebuilt/windows-x86_64/bin"
    foreach ($abi in @('armeabi-v7a','arm64-v8a','x86_64')) {
        $target=switch($abi) {
            'armeabi-v7a' {'armv7a-linux-androideabi30'}
            'arm64-v8a' {'aarch64-linux-android30'}
            'x86_64' {'x86_64-linux-android30'}
        }
        New-Item -ItemType Directory -Force "$out/native/lib/$abi" | Out-Null
        Invoke-Checked "$llvm/clang.exe" @("--target=$target",'-shared','-fPIC','-O2','-Wall','-Wextra','-Wno-unused-parameter',"$PSScriptRoot/client/native/audio.c",'-laaudio','-o',"$out/native/lib/$abi/libstreamaudio.so")
    }
    Invoke-Checked "$Jdk/bin/jar.exe" @('uf',"$out/unsigned.apk",'-C',"$out/native",'lib')
}
else { Write-Warning 'NDK unavailable: AudioTrack fallback only (may have higher device latency).' }
Invoke-Checked "$BuildTools/zipalign.exe" @('-f','4',"$out/unsigned.apk","$out/aligned.apk")
if (!(Test-Path "$out/debug.keystore")) {
    Invoke-Checked "$Jdk/bin/keytool.exe" @('-genkeypair','-keystore',"$out/debug.keystore",'-storepass','android','-keypass','android','-alias','androiddebugkey','-dname','CN=Livingroom Experiment','-keyalg','RSA','-validity','3650','-noprompt')
}
Invoke-Checked "$Jdk/bin/java.exe" @('-jar',"$BuildTools/lib/apksigner.jar",'sign','--ks',"$out/debug.keystore",'--ks-pass','pass:android','--out',"$out/streamlab.apk","$out/aligned.apk")
Invoke-Checked "$Jdk/bin/java.exe" @('-jar',"$BuildTools/lib/apksigner.jar",'verify',"$out/streamlab.apk")
Write-Output "APK: $out/streamlab.apk"
