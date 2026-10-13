param([ValidateSet('Install','Remove','Uninstall')][string]$Action='Install',[string]$RenderGpu,[int]$WaitForProcess=0)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$experiment=Join-Path $root 'local/game-space'
$record=Join-Path $experiment 'driver-operation.json'
$leasePath=Join-Path $experiment 'display-driver-lease.json'
$packageLease=Join-Path $experiment 'driver-package-lease.json'
$driverDir=Join-Path $experiment 'vdd-control/SignedDrivers/x86/VDD'
$devcon=Join-Path $experiment 'vdd-control/Dependencies/devcon.exe'
$configDir='C:\VirtualDisplayDriver'
function OwnedDriverDevices {
    @(Get-PnpDevice -Class Display -PresentOnly | Where-Object {
        $hardwareIds=(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds' -ErrorAction SilentlyContinue).Data
        @($hardwareIds) -contains 'Root\MttVDD'
    })
}
try {
    if(![Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Administrator elevation required'}
    if($Action -eq 'Uninstall'){
        if(Test-Path -LiteralPath $leasePath){throw 'Recover the owned display before uninstalling'}
        if(@(OwnedDriverDevices).Count){throw 'A virtual display device still uses this driver'}
        $sourceInf=Join-Path $driverDir 'MttVDD.inf'
        $sourceHash=(Get-FileHash -LiteralPath $sourceInf -Algorithm SHA256).Hash
        if((Get-AuthenticodeSignature -LiteralPath (Join-Path $driverDir 'mttvdd.cat')).Status -ne 'Valid'){throw 'Driver signature is not valid'}
        # Exact signed package identity; never uninstall a guessed OEM number or force in-use deletion.
        $packages=@(Get-ChildItem -LiteralPath "$env:windir\INF" -Filter 'oem*.inf' | Where-Object {(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash -eq $sourceHash})
        if($packages.Count -gt 1){throw 'Multiple matching packages; refusing ambiguous deletion'}
        if($packages.Count -eq 1){
            $package=$packages[0].Name
            & "$env:windir\System32\pnputil.exe" /delete-driver $package *> (Join-Path $experiment 'driver-uninstall.log')
            if($LASTEXITCODE -ne 0 -or (Test-Path -LiteralPath $packages[0].FullName)){throw 'Driver package removal not confirmed; see driver-uninstall.log'}
        }
        if(Test-Path -LiteralPath $packageLease){Remove-Item -LiteralPath $packageLease}
        @{action=$Action;success=$true;note='Owned virtual-display package removed from Windows driver store'} | ConvertTo-Json | Set-Content -LiteralPath $record -Encoding UTF8
    }elseif($Action -eq 'Install'){
        if(Test-Path -LiteralPath $leasePath){throw 'Existing experiment lease; remove or inspect first'}
        if(Test-Path -LiteralPath $configDir){
            $previous=if(Test-Path -LiteralPath $record){Get-Content -LiteralPath $record -Raw | ConvertFrom-Json}else{$null}
            if($previous.action -eq 'Install' -and !$previous.success -and $previous.error -match 'AppendChild' -and @(Get-ChildItem -LiteralPath $configDir -Force).Count -eq 0){Remove-Item -LiteralPath $configDir}
            else{throw 'Existing VDD configuration; refusing to overwrite'}
        }
        if(@(OwnedDriverDevices).Count){throw 'Existing VDD device; refusing to modify'}
        foreach($signed in @($devcon,(Join-Path $driverDir 'mttvdd.cat'))){if((Get-AuthenticodeSignature -LiteralPath $signed).Status -ne 'Valid'){throw 'Driver/tool signature is not valid'}}
        [xml]$settings=Get-Content -LiteralPath (Join-Path $driverDir 'vdd_settings.xml') -Raw
        if(!$RenderGpu){
            $nvencGpus=@(Get-CimInstance Win32_VideoController | Where-Object {$_.Name -match '^NVIDIA '})
            if($nvencGpus.Count -ne 1){throw 'Specify RenderGpu explicitly for this NVENC experiment'}
            $RenderGpu=$nvencGpus[0].Name
        }
        $settings.vdd_settings.gpu.friendlyname=$RenderGpu
        $globalNode=$settings.SelectSingleNode('/vdd_settings/global')
        $resolutionsNode=$settings.SelectSingleNode('/vdd_settings/resolutions')
        $globalNode.RemoveAll()
        $rate=$settings.CreateElement('g_refresh_rate');$rate.InnerText='60';[void]$globalNode.AppendChild($rate)
        $resolutionsNode.RemoveAll()
        $resolution=$settings.CreateElement('resolution')
        foreach($entry in @(@('width','1920'),@('height','1080'),@('refresh_rate','60'))){$node=$settings.CreateElement($entry[0]);$node.InnerText=$entry[1];[void]$resolution.AppendChild($node)}
        [void]$resolutionsNode.AppendChild($resolution)
        @{version=1;config_created=$true;instance=$null;driver_package=$null} | ConvertTo-Json | Set-Content -LiteralPath $leasePath -Encoding UTF8
        New-Item -ItemType Directory -Path $configDir | Out-Null
        $settings.Save((Join-Path $configDir 'vdd_settings.xml'))
        & $devcon install (Join-Path $driverDir 'MttVDD.inf') 'Root\MttVDD' *> (Join-Path $experiment 'driver-install.log')
        $installCode=$LASTEXITCODE
        $devices=@(OwnedDriverDevices)
        if($devices.Count -ne 1){throw "Device creation unconfirmed; inspect driver-install.log (exit $installCode)"}
        $device=$devices[0]
        $package=(Get-PnpDeviceProperty -InstanceId $device.InstanceId -KeyName 'DEVPKEY_Device_DriverInfPath').Data
        @{version=1;driver_package=$package;inf_sha256=(Get-FileHash -LiteralPath (Join-Path $driverDir 'MttVDD.inf')).Hash} | ConvertTo-Json | Set-Content -LiteralPath $packageLease -Encoding UTF8
        @{version=1;config_created=$true;instance=$device.InstanceId;driver_package=$package} | ConvertTo-Json | Set-Content -LiteralPath $leasePath -Encoding UTF8
        if($installCode -notin @(0,1) -or $device.Status -ne 'OK'){throw "Driver not ready: exit $installCode / $($device.Status)"}
        @{action=$Action;success=$true;instance=$device.InstanceId;reboot_required=($installCode -eq 1)} | ConvertTo-Json | Set-Content -LiteralPath $record -Encoding UTF8
    } else {
        if($WaitForProcess -gt 0 -and (Get-Process -Id $WaitForProcess -ErrorAction SilentlyContinue)){
            Wait-Process -Id $WaitForProcess -Timeout 50 -ErrorAction Stop
        }
        if(!(Test-Path -LiteralPath $leasePath)){throw 'No experiment ownership lease; refusing removal'}
        $lease=Get-Content -LiteralPath $leasePath -Raw | ConvertFrom-Json
        if(!$lease.instance){
            # devcon may create a device before installation writes its final ownership record.
            $partial=@(OwnedDriverDevices)
            if($partial.Count -gt 1){throw 'Partial installation has ambiguous devices; refusing removal'}
            if($partial.Count -eq 1){
                $lease.instance=$partial[0].InstanceId
                $lease.driver_package=(Get-PnpDeviceProperty -InstanceId $lease.instance -KeyName 'DEVPKEY_Device_DriverInfPath').Data
                $installedInf=Join-Path "$env:windir\INF" $lease.driver_package
                if((Get-FileHash -LiteralPath $installedInf).Hash -ne (Get-FileHash -LiteralPath (Join-Path $driverDir 'MttVDD.inf')).Hash){throw 'Partial installation package identity changed'}
            }
        }
        if($lease.instance -and (Get-PnpDevice -InstanceId $lease.instance -PresentOnly -ErrorAction SilentlyContinue)){
            $owned=@(OwnedDriverDevices | Where-Object {$_.InstanceId -eq $lease.instance})
            if($owned.Count -ne 1){throw 'Owned hardware identity not confirmed'}
            $package=(Get-PnpDeviceProperty -InstanceId $lease.instance -KeyName 'DEVPKEY_Device_DriverInfPath').Data
            if($package -ne $lease.driver_package){throw 'Owned driver package changed'}
            @{version=1;driver_package=$package;inf_sha256=(Get-FileHash -LiteralPath (Join-Path $driverDir 'MttVDD.inf')).Hash} | ConvertTo-Json | Set-Content -LiteralPath $packageLease -Encoding UTF8
            & $devcon remove "@$($lease.instance)" *> (Join-Path $experiment 'driver-remove.log')
            if($LASTEXITCODE -notin @(0,1)){throw 'Device removal failed'}
            if(Get-PnpDevice -InstanceId $lease.instance -PresentOnly -ErrorAction SilentlyContinue){throw 'Device remains present'}
        }
        if($lease.config_created -and (Test-Path -LiteralPath $configDir)){
            if(Test-Path -LiteralPath (Join-Path $configDir 'vdd_settings.xml')){Remove-Item -LiteralPath (Join-Path $configDir 'vdd_settings.xml')}
            if(@(Get-ChildItem -LiteralPath $configDir -Force).Count -eq 0){Remove-Item -LiteralPath $configDir}
        }
        Remove-Item -LiteralPath $leasePath
        @{action=$Action;success=$true;note='Owned device removed; staged signed driver package retained in Windows driver store'} | ConvertTo-Json | Set-Content -LiteralPath $record -Encoding UTF8
    }
}catch{
    @{action=$Action;success=$false;error=$_.Exception.Message} | ConvertTo-Json | Set-Content -LiteralPath $record -Encoding UTF8
    exit 1
}
