param([ValidateSet('Start','Stop','Inspect')][string]$Action='Inspect',[long]$Window=0,[string]$StateFile,[int]$OwnerPid=0)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
function BigPicture {
    @(Get-Process steamwebhelper -ErrorAction SilentlyContinue | Where-Object {$_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -match '大屏幕|Big Picture|大螢幕'})
}
if($Action -eq 'Start') {
    $steamExe=(Get-ItemProperty 'HKCU:\Software\Valve\Steam' -ErrorAction Stop).SteamExe
    if(!(Test-Path -LiteralPath $steamExe)){throw 'Steam executable unavailable'}
    if($StateFile){@{window=0;ownerPid=$OwnerPid} | ConvertTo-Json -Compress | Set-Content -LiteralPath $StateFile -Encoding UTF8}
    Start-Process -FilePath $steamExe -ArgumentList 'steam://open/bigpicture' -WindowStyle Normal
    $deadline=[DateTime]::UtcNow.AddSeconds(25)
    do {
        $ready=BigPicture | Select-Object -First 1
        if($null -ne $ready){
            if($StateFile){@{window=$ready.MainWindowHandle.ToInt64();ownerPid=$OwnerPid} | ConvertTo-Json -Compress | Set-Content -LiteralPath $StateFile -Encoding UTF8}
            @{ready=$true;window=$ready.MainWindowHandle.ToInt64();window_pid=$ready.Id} | ConvertTo-Json -Compress; exit 0
        }
        Start-Sleep -Milliseconds 100
    }while([DateTime]::UtcNow -lt $deadline)
    throw 'Steam Big Picture did not become ready; check login/dialog on PC'
}
if($Action -eq 'Stop') {
    if($StateFile -and (Test-Path -LiteralPath $StateFile)){
        $saved=Get-Content -LiteralPath $StateFile -Raw | ConvertFrom-Json
        if($saved.ownerPid -ne $OwnerPid -and (Get-Process -Id $saved.ownerPid -ErrorAction SilentlyContinue)){throw 'Steam window belongs to a live host'}
        if($Window -eq 0){$Window=$saved.window}
    }
    if($Window -eq 0){
        $deadline=[DateTime]::UtcNow.AddSeconds(5)
        do {$candidate=BigPicture | Select-Object -First 1;if($null -ne $candidate){$Window=$candidate.MainWindowHandle.ToInt64();break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow -lt $deadline)
        if($Window -eq 0){throw 'Steam launch requested but target window unconfirmed; retry cleanup after handling Steam on PC'}
    }
    $owned=BigPicture | Where-Object {$_.MainWindowHandle.ToInt64() -eq $Window} | Select-Object -First 1
    if($null -ne $owned){if(!$owned.CloseMainWindow()){throw 'Steam refused normal window close'}}
    $deadline=[DateTime]::UtcNow.AddSeconds(5)
    while(@(BigPicture | Where-Object {$_.MainWindowHandle.ToInt64() -eq $Window}).Count -ne 0){
        if([DateTime]::UtcNow -ge $deadline){throw 'Steam Big Picture close not confirmed'}
        Start-Sleep -Milliseconds 100
    }
    if($StateFile -and (Test-Path -LiteralPath $StateFile)){Remove-Item -LiteralPath $StateFile}
    @{stopped=$true} | ConvertTo-Json -Compress; exit 0
}
@{ready=@(BigPicture).Count -gt 0} | ConvertTo-Json -Compress
