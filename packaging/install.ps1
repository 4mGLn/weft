[CmdletBinding()]
param(
    [string]$Prefix = $(if ($env:WEFT_PREFIX) { $env:WEFT_PREFIX } else { Join-Path $env:LOCALAPPDATA 'Weft' })
)

$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot 'bin/weft.exe'
$destination = Join-Path $Prefix 'bin/weft.exe'
if (-not (Test-Path -LiteralPath $source)) { throw "missing runtime binary: $source" }
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination) | Out-Null
Copy-Item -LiteralPath $source -Destination $destination -Force
Write-Output "installed $destination"
Write-Output "Add $(Split-Path -Parent $destination) to PATH if it is not already present."
