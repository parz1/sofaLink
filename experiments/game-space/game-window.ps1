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
    if(Test-Path -LiteralPath $leaseFile){throw 'Existing game window lease; restore before moving again'}
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
    if($lease.pid -ne $game.Id -or $lease.window -ne $window.ToInt64() -or !$sameStart){throw 'Game identity changed; refusing to restore another process'}
    if(![SofaGameWindow]::SetWindowPos($window,[IntPtr]::Zero,$lease.x,$lease.y,$lease.width,$lease.height,0x14)){throw 'Window restore failed'}
    Remove-Item -LiteralPath $leaseFile
}
if(![SofaGameWindow]::GetWindowRect($window,[ref]$rect)){throw 'Cannot verify final window position'}
@{pid=$game.Id;window=$window.ToInt64();foreground=([SofaGameWindow]::GetForegroundWindow() -eq $window);x=$rect.left;y=$rect.top;width=($rect.right-$rect.left);height=($rect.bottom-$rect.top)} | ConvertTo-Json
