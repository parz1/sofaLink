$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$tools=Join-Path $root 'local/tools'
New-Item -ItemType Directory -Force $tools | Out-Null
function Download-Archive([string]$url,[string]$name,[string]$folder,[string]$sha1) {
    if(Test-Path "$tools/$folder") {return}
    Invoke-WebRequest $url -OutFile "$tools/$name.zip"
    if($sha1 -and (Get-FileHash "$tools/$name.zip" -Algorithm SHA1).Hash -ne $sha1) {throw "Checksum mismatch: $name"}
    Expand-Archive -LiteralPath "$tools/$name.zip" -DestinationPath "$tools/$folder"
}
# Official GStreamer Python distribution; only these packages are needed by the experiment.
& python -m pip install --target "$tools/gst-python" gstreamer-libs==1.28.7 gstreamer-plugins==1.28.7 gstreamer-plugins-restricted==1.28.7 gstreamer-cli==1.28.7 gstreamer-python==1.28.7
if($LASTEXITCODE -ne 0){throw 'GStreamer installation failed'}
Download-Archive 'https://aka.ms/download-jdk/microsoft-jdk-17.0.20.1-windows-x64.zip' 'jdk' 'jdk' ''
Download-Archive 'https://dl.google.com/android/repository/platform-35_r02.zip' 'android-platform' 'android-platform' '0bb560a90a7a2cbd0dd8348224d518b638fe7949'
Download-Archive 'https://dl.google.com/android/repository/build-tools_r35_windows.zip' 'android-build' 'android-build' 'af059bb67cf7786f45ee0db85e2d24985df1b4b6'
if(!(Test-Path "$tools/platform-tools/adb.exe")) {
    Invoke-WebRequest 'https://dl.google.com/android/repository/platform-tools-latest-windows.zip' -OutFile "$tools/platform-tools.zip"
    Expand-Archive -LiteralPath "$tools/platform-tools.zip" -DestinationPath $tools -Force
}
Write-Host 'Tools prepared under local/tools. No system PATH changes.'
