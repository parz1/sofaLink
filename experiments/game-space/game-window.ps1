param([ValidateSet('Inspect','Move','Restore')][string]$Action='Inspect',[string]$ProcessName='LostJudgment',[long]$MonitorHandle=0)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$leaseFile=Join-Path $root 'local/game-space/game-window-lease.json'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class SofaGameWindow {
    [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
    [StructLayout(LayoutKind.Sequential)] public struct MonitorInfo {public uint size;public Rect monitor,work;public uint flags;}
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr window,IntPtr after,int x,int y,int width,int height,uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr window);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern bool GetMonitorInfo(IntPtr monitor,ref MonitorInfo info);
}
'@
$game=@(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Where-Object {$_.MainWindowHandle -ne 0})
if($game.Count -ne 1){throw 'Expected exactly one running game window'}
$game=$game[0]
$window=$game.MainWindowHandle
$rect=[SofaGameWindow+Rect]::new()
if(![SofaGameWindow]::GetWindowRect($window,[ref]$rect)){throw 'Cannot query game window'}
if($Action -eq 'Move'){
    if(Test-Path -LiteralPath $leaseFile){
        $previous=Get-Content -LiteralPath $leaseFile -Raw | ConvertFrom-Json
        $previousProcess=Get-Process -Id $previous.pid -ErrorAction SilentlyContinue
        $sameProcess=$previousProcess -and $previousProcess.StartTime.ToUniversalTime().Ticks -eq ([DateTime]$previous.process_start).ToUniversalTime().Ticks
        if($sameProcess){throw 'Existing live game window lease; restore before moving again'}
        # The old process exited. Archive its record; never restore geometry onto the new game.
        $archive=Join-Path $root "local/game-space/expired-window-lease-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()).json"
        Move-Item -LiteralPath $leaseFile -Destination $archive
    }
    if($MonitorHandle -le 0){throw 'Explicit monitor handle required'}
    $monitor=[SofaGameWindow+MonitorInfo]::new();$monitor.size=[Runtime.InteropServices.Marshal]::SizeOf($monitor)
    if(![SofaGameWindow]::GetMonitorInfo([IntPtr]$MonitorHandle,[ref]$monitor)){throw 'Target monitor unavailable'}
    if(($monitor.flags -band 1) -ne 0){throw 'Experiment refuses moving game onto primary screen'}
    @{pid=$game.Id;process_start=$game.StartTime.ToUniversalTime().ToString('o');window=$window.ToInt64();x=$rect.left;y=$rect.top;width=($rect.right-$rect.left);height=($rect.bottom-$rect.top)} | ConvertTo-Json | Set-Content -LiteralPath $leaseFile -Encoding UTF8
    $target=$monitor.monitor
    # SWP_NOZORDER | SWP_NOACTIVATE: moving the game never grabs local focus.
    if(![SofaGameWindow]::SetWindowPos($window,[IntPtr]::Zero,$target.left,$target.top,$target.right-$target.left,$target.bottom-$target.top,0x14)){throw 'Window move failed; lease retained for restoration'}
}elseif($Action -eq 'Restore'){
    $lease=Get-Content -LiteralPath $leaseFile -Raw | ConvertFrom-Json
    $sameStart=([DateTime]$lease.process_start).ToUniversalTime().Ticks -eq $game.StartTime.ToUniversalTime().Ticks
    if($lease.pid -ne $game.Id -or !$sameStart){throw 'Game identity changed; refusing to restore another process'}
    if($lease.window -ne $window.ToInt64() -and [SofaGameWindow]::IsWindow([IntPtr]$lease.window)){throw 'Original game window still exists; replacement is ambiguous'}
    $topology=& "$PSScriptRoot/display-probe.ps1" | ConvertFrom-Json
    $primary=@($topology.displays | Where-Object primary)
    if($primary.Count -ne 1){throw 'Primary display not confirmed'}
    $primary=$primary[0]
    $width=[Math]::Min($lease.width,$primary.width);$height=[Math]::Min($lease.height,$primary.height)
    $x=[Math]::Clamp([int]$lease.x,[int]$primary.x,[int]($primary.x+$primary.width-$width))
    $y=[Math]::Clamp([int]$lease.y,[int]$primary.y,[int]($primary.y+$primary.height-$height))
    if(![SofaGameWindow]::SetWindowPos($window,[IntPtr]::Zero,$x,$y,$width,$height,0x14)){throw 'Window restore failed'}
    $stable=0
    for($attempt=0;$attempt -lt 20;$attempt++){
        Start-Sleep -Milliseconds 50
        if(![SofaGameWindow]::GetWindowRect($window,[ref]$rect)){throw 'Cannot verify restored window'}
        $inside=$rect.left -ge $primary.x -and $rect.top -ge $primary.y -and $rect.right -le ($primary.x+$primary.width) -and $rect.bottom -le ($primary.y+$primary.height)
        $stable=if($inside){$stable+1}else{0};if($stable -ge 6){break}
    }
    if($stable -lt 6){throw 'Game did not remain on primary display; display lease retained'}
    Remove-Item -LiteralPath $leaseFile
}
if(![SofaGameWindow]::GetWindowRect($window,[ref]$rect)){throw 'Cannot verify final window position'}
@{pid=$game.Id;window=$window.ToInt64();foreground=([SofaGameWindow]::GetForegroundWindow() -eq $window);x=$rect.left;y=$rect.top;width=($rect.right-$rect.left);height=($rect.bottom-$rect.top)} | ConvertTo-Json
