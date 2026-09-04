[CmdletBinding()]
param(
    [string]$Prefix = $(if ($env:WEFT_PREFIX) { $env:WEFT_PREFIX } else { Join-Path $env:LOCALAPPDATA 'Weft' })
)

$ErrorActionPreference = 'Stop'
$destination = Join-Path $Prefix 'bin/weft.exe'
if (Test-Path -LiteralPath $destination) { Remove-Item -LiteralPath $destination -Force }
Write-Output "removed $destination"
Write-Output 'Weft state directories are retained; remove an explicit state directory separately if intended.'
