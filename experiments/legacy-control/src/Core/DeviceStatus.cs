namespace LivingroomHub.Core;

public sealed record DeviceStatus(string Model, string AndroidVersion, string Package, bool? MoonlightForeground);
public sealed class DeviceException(string code, string message) : Exception(message)
{
    public string Code { get; } = code;
}
