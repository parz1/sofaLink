// Control-plane COM bridge only; no PCM processing. IPolicyConfig is an
// undocumented Windows interface, also used by Sunshine. Keep failures explicit.
using System;
using System.Runtime.InteropServices;

namespace Livingroom.Audio {
    [ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class Enumerator {}
    [ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IDevices {
        void EnumAudioEndpoints(int flow, uint state, out IntPtr collection);
        void GetDefaultAudioEndpoint(int flow, int role, out IDevice device);
        void GetDevice([MarshalAs(UnmanagedType.LPWStr)] string id, out IDevice device);
        void RegisterEndpointNotificationCallback(IntPtr callback);
        void UnregisterEndpointNotificationCallback(IntPtr callback);
    }
    [ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IDevice {
        void Activate(ref Guid iid, uint context, IntPtr parameters, out IntPtr result);
        void OpenPropertyStore(uint mode, out IntPtr properties);
        void GetId([MarshalAs(UnmanagedType.LPWStr)] out string id);
        void GetState(out uint state);
    }
    [ComImport, Guid("870AF99C-171D-4F9E-AF0D-E63DF40C2BC9")] class Policy {}
    [ComImport, Guid("F8679F50-850A-41CF-9C72-430F290290C8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IPolicy {
        void GetMixFormat();
        void GetDeviceFormat();
        void ResetDeviceFormat();
        void SetDeviceFormat();
        void GetProcessingPeriod();
        void SetProcessingPeriod();
        void GetShareMode();
        void SetShareMode();
        void GetPropertyValue();
        void SetPropertyValue();
        void SetDefaultEndpoint([MarshalAs(UnmanagedType.LPWStr)] string id, int role);
        void SetEndpointVisibility();
    }
    [ComImport, Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IVolume {
        void RegisterControlChangeNotify(IntPtr callback);
        void UnregisterControlChangeNotify(IntPtr callback);
        void GetChannelCount(out uint count);
        void SetMasterVolumeLevel(float level, ref Guid context);
        void SetMasterVolumeLevelScalar(float level, ref Guid context);
        void GetMasterVolumeLevel(out float level);
        void GetMasterVolumeLevelScalar(out float level);
        void SetChannelVolumeLevel(uint channel, float level, ref Guid context);
        void SetChannelVolumeLevelScalar(uint channel, float level, ref Guid context);
        void GetChannelVolumeLevel(uint channel, out float level);
        void GetChannelVolumeLevelScalar(uint channel, out float level);
        void SetMute([MarshalAs(UnmanagedType.Bool)] bool mute, ref Guid context);
        void GetMute([MarshalAs(UnmanagedType.Bool)] out bool mute);
    }
    public static class Endpoints {
        static IVolume Volume(string id) {
            var devices=(IDevices)new Enumerator(); IDevice device=null; IntPtr raw=IntPtr.Zero;
            try {
                devices.GetDevice(id,out device); var iid=new Guid("5CDF2C82-841E-4546-9722-0CF74078229A");
                device.Activate(ref iid,23,IntPtr.Zero,out raw); return (IVolume)Marshal.GetObjectForIUnknown(raw);
            } finally { if(raw!=IntPtr.Zero) Marshal.Release(raw); if(device!=null) Marshal.ReleaseComObject(device); Marshal.ReleaseComObject(devices); }
        }
        public static bool Muted(string id) {
            var volume=Volume(id); try { bool value; volume.GetMute(out value); return value; } finally {Marshal.ReleaseComObject(volume);}
        }
        public static float Level(string id) {
            var volume=Volume(id); try { float value; volume.GetMasterVolumeLevelScalar(out value); return value; } finally {Marshal.ReleaseComObject(volume);}
        }
        public static void Mute(string id, bool value) {
            var volume=Volume(id); var context=new Guid("1B56A67B-3B2C-44A5-BAAF-6D6CAB3CE4E9");
            try { volume.SetMute(value,ref context); if(Muted(id)!=value) throw new InvalidOperationException("Mute was not adopted"); } finally {Marshal.ReleaseComObject(volume);}
        }
        public static string Canonical(string id) {
            var devices = (IDevices)new Enumerator();
            IDevice device = null;
            try { devices.GetDevice(id, out device); string canonical; device.GetId(out canonical); return canonical; }
            finally { if(device != null) Marshal.ReleaseComObject(device); Marshal.ReleaseComObject(devices); }
        }
        public static string Default(int role) {
            var devices = (IDevices)new Enumerator();
            IDevice device = null;
            try { devices.GetDefaultAudioEndpoint(0, role, out device); string id; device.GetId(out id); return id; }
            finally { if(device != null) Marshal.ReleaseComObject(device); Marshal.ReleaseComObject(devices); }
        }
        public static bool Active(string id) {
            var devices = (IDevices)new Enumerator();
            IDevice device = null;
            try { devices.GetDevice(id, out device); uint state; device.GetState(out state); return state == 1; }
            finally { if(device != null) Marshal.ReleaseComObject(device); Marshal.ReleaseComObject(devices); }
        }
        public static void Set(string id, int role) {
            if(!Active(id)) throw new InvalidOperationException("Audio endpoint is not active: " + id);
            var policy = (IPolicy)new Policy();
            try { policy.SetDefaultEndpoint(id, role); if(!String.Equals(Default(role), id, StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("Audio route was not adopted"); }
            finally { Marshal.ReleaseComObject(policy); }
        }
    }
}
