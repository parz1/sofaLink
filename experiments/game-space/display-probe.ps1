param([string]$OutputFile)
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class SofaDisplayProbe {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int left,top,right,bottom; }
    [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct MonitorInfo {
        public int size; public Rect monitor,work; public uint flags;
        [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)] public string device;
    }
    public class Display { public long handle; public string device; public bool primary; public int x,y,width,height; }
    delegate bool Callback(IntPtr monitor,IntPtr dc,ref Rect rect,IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumDisplayMonitors(IntPtr dc,IntPtr clip,Callback callback,IntPtr data);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern bool GetMonitorInfo(IntPtr monitor,ref MonitorInfo info);
    public static Display[] Read() {
        var result=new List<Display>();
        Callback callback=delegate(IntPtr handle,IntPtr dc,ref Rect rect,IntPtr data) {
            var info=new MonitorInfo();info.size=Marshal.SizeOf(typeof(MonitorInfo));
            if(!GetMonitorInfo(handle,ref info))throw new InvalidOperationException("Monitor query failed");
            result.Add(new Display {handle=handle.ToInt64(),device=info.device,primary=(info.flags&1)!=0,x=rect.left,y=rect.top,width=rect.right-rect.left,height=rect.bottom-rect.top});
            return true;
        };
        if(!EnumDisplayMonitors(IntPtr.Zero,IntPtr.Zero,callback,IntPtr.Zero))throw new InvalidOperationException("Monitor enumeration failed");
        return result.ToArray();
    }
}
'@
$result=@{version=1;utc=[DateTime]::UtcNow.ToString('o');displays=@([SofaDisplayProbe]::Read())}
$json=$result | ConvertTo-Json -Depth 5
if($OutputFile){$json | Set-Content -LiteralPath $OutputFile -Encoding UTF8}
$json
