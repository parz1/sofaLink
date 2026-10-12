"""Windows capture target validation; never silently replace a missing monitor."""
import ctypes
from ctypes import wintypes


def monitor_bounds(handle):
    class MonitorInfo(ctypes.Structure):
        _fields_ = [("size", wintypes.DWORD), ("monitor", wintypes.RECT),
                    ("work", wintypes.RECT), ("flags", wintypes.DWORD)]

    api = ctypes.WinDLL("user32", use_last_error=True)
    query = api.GetMonitorInfoW
    query.argtypes = [wintypes.HANDLE, ctypes.POINTER(MonitorInfo)]
    query.restype = wintypes.BOOL
    info = MonitorInfo()
    info.size = ctypes.sizeof(info)
    if not query(handle, ctypes.byref(info)):
        raise RuntimeError("Selected monitor is unavailable; refusing primary-display fallback")
    rect = info.monitor
    return {"handle": handle, "x": rect.left, "y": rect.top,
            "width": rect.right - rect.left, "height": rect.bottom - rect.top,
            "primary": bool(info.flags & 1)}
