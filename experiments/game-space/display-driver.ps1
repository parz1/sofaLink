param([ValidateSet('Install','Remove')][string]$Action='Install')
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$experiment=Join-Path $root 'local/game-space'
$record=Join-Path $experiment 'driver-operation.json'
$leasePath=Join-Path $experiment 'display-driver-lease.json'
$driverDir=Join-Path $experiment 'vdd-control/SignedDrivers/x86/VDD'
$devcon=Join-Path $experiment 'vdd-control/Dependencies/devcon.exe'
$configDir='C:\VirtualDisplayDriver'
try {
    if(![Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Administrator elevation required'}
    if($Action -eq 'Install'){
        if(Test-Path -LiteralPath $leasePath){throw 'Existing experiment lease; remove or inspect first'}
        if(Test-Path -LiteralPath $configDir){throw 'Existing VDD configuration; refusing to overwrite'}
        if(@(Get-PnpDevice -Class Display | Where-Object {$_.InstanceId -like 'ROOT\MTTVDD\*'}).Count){throw 'Existing VDD device; refusing to modify'}
        foreach($signed in @($devcon,(Join-Path $driverDir 'mttvdd.cat'))){if((Get-AuthenticodeSignature -LiteralPath $signed).Status -ne 'Valid'){throw 'Driver/tool signature is not valid'}}
        New-Item -ItemType Directory -Path $configDir | Out-Null
        [xml]$settings=Get-Content -LiteralPath (Join-Path $driverDir 'vdd_settings.xml') -Raw
        $settings.vdd_settings.gpu.friendlyname='NVIDIA GeForce RTX 4070 SUPER'
        $settings.vdd_settings.global.RemoveAll()
        $rate=$settings.CreateElement('g_refresh_rate');$rate.InnerText='60';[void]$settings.vdd_settings.global.AppendChild($rate)
        $settings.vdd_settings.resolutions.RemoveAll()
        $resolution=$settings.CreateElement('resolution')
        foreach($entry in @(@('width','1920'),@('height','1080'),@('refresh_rate','60'))){$node=$settings.CreateElement($entry[0]);$node.InnerText=$entry[1];[void]$resolution.AppendChild($node)}
        [void]$settings.vdd_settings.resolutions.AppendChild($resolution)
        $settings.Save((Join-Path $configDir 'vdd_settings.xml'))
        @{version=1;config_created=$true;instance=$null;driver_package=$null} | ConvertTo-Json | Set-Content -LiteralPath $leasePath -Encoding UTF8
        & $devcon install (Join-Path $driverDir 'MttVDD.inf') 'Root\MttVDD' *> (Join-Path $experiment 'driver-install.log')
        $installCode=$LASTEXITCODE
        $devices=@(Get-PnpDevice -Class Display | Where-Object {$_.InstanceId -like 'ROOT\MTTVDD\*'})
        if($devices.Count -ne 1){throw "Device creation unconfirmed; inspect driver-install.log (exit $installCode)"}
        $device=$devices[0]
        $package=(Get-PnpDeviceProperty -InstanceId $device.InstanceId -KeyName 'DEVPKEY_Device_DriverInfPath').Data
        @{version=1;config_created=$true;instance=$device.InstanceId;driver_package=$package} | ConvertTo-Json | Set-Content -LiteralPath $leasePath -Encoding UTF8
        if($installCode -notin @(0,1) -or $device.Status -ne 'OK'){throw "Driver not ready: exit $installCode / $($device.Status)"}
        @{action=$Action;success=$true;instance=$device.InstanceId;reboot_required=($installCode -eq 1)} | ConvertTo-Json | Set-Content -LiteralPath $record -Encoding UTF8
    } else {
        if(!(Test-Path -LiteralPath $leasePath)){throw 'No experiment ownership lease; refusing removal'}
        $lease=Get-Content -LiteralPath $leasePath -Raw | ConvertFrom-Json
        if($lease.instance){
            if($lease.instance -notmatch '^ROOT\\MTTVDD\\[0-9]+$'){throw 'Unexpected device identity'}
            & $devcon remove "@$($lease.instance)" *> (Join-Path $experiment 'driver-remove.log')
            if($LASTEXITCODE -notin @(0,1)){throw 'Device removal failed'}
            if(Get-PnpDevice -InstanceId $lease.instance -PresentOnly -ErrorAction SilentlyContinue){throw 'Device remains present'}
        }
        if($lease.config_created){
            Remove-Item -LiteralPath (Join-Path $configDir 'vdd_settings.xml') -ErrorAction SilentlyContinue
            if(@(Get-ChildItem -LiteralPath $configDir -Force).Count -eq 0){Remove-Item -LiteralPath $configDir}
        }
        Remove-Item -LiteralPath $leasePath
        @{action=$Action;success=$true;note='Owned device removed; staged signed driver package retained in Windows driver store'} | ConvertTo-Json | Set-Content -LiteralPath $record -Encoding UTF8
    }
}catch{
    @{action=$Action;success=$false;error=$_.Exception.Message} | ConvertTo-Json | Set-Content -LiteralPath $record -Encoding UTF8
    exit 1
}
