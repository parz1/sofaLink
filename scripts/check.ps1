$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
function Invoke-Cargo([string[]]$Arguments) {
    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) { throw "cargo $($Arguments -join ' ') failed" }
}
Push-Location $repoRoot
try {
    $metadataText = & cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Unable to inspect workspace' }
    $metadata = $metadataText | ConvertFrom-Json
    $policy = @{
        'sofalink-core' = @()
        'sofalink-simulator' = @('sofalink-core')
        'sofalink-input' = @('sofalink-core')
        'sofalink-emulator' = @('sofalink-core','sofalink-input')
        'sofalink-tui' = @('sofalink-core', 'sofalink-simulator', 'sofalink-emulator')
    }
    $workspaceNames = @($metadata.packages.name)
    foreach ($package in $metadata.packages) {
        if (!$policy.ContainsKey($package.name)) { throw "Define dependency policy for $($package.name) first" }
        foreach ($dependency in $package.dependencies) {
            if ($dependency.name -in $workspaceNames -and $dependency.name -notin $policy[$package.name]) {
                throw "Invalid dependency direction: $($package.name) -> $($dependency.name)"
            }
            if ($package.name -eq 'sofalink-core' -and $dependency.name -ne 'serde') {
                throw "Core must remain independent of UI/backend/I/O: $($dependency.name)"
            }
        }
    }
    Write-Host 'Workspace dependency boundaries verified.'
    Invoke-Cargo @('fmt', '--all', '--check')
    Invoke-Cargo @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
    Invoke-Cargo @('test', '--workspace', '--locked')
    Invoke-Cargo @('run', '--locked', '-p', 'sofalink-simulator', '--', 'check')
}
finally { Pop-Location }
