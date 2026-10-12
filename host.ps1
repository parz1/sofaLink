param([string]$Bind='0.0.0.0:47990',[string]$Python,[switch]$Diagnostics,[switch]$ResetPairing)
$ErrorActionPreference='Stop'
Push-Location $PSScriptRoot
try {
    $hostArgs=@('--root',$PSScriptRoot,'--bind',$Bind)
    if($Python){$hostArgs+=@('--python',$Python)}
    if($Diagnostics){$hostArgs+='--diagnostics'}
    if($ResetPairing){$hostArgs+='--reset-pairing'}
    & cargo run --locked -p sofalink-host -- @hostArgs
    if($LASTEXITCODE -ne 0){throw 'sofaLink host exited with an error'}
}finally {Pop-Location}
