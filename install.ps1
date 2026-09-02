# Download a verified Windows runtime release and invoke its archive installer.
$ErrorActionPreference = 'Stop'
$repository = if ($env:WEFT_REPOSITORY) { $env:WEFT_REPOSITORY } else { '4mGLn/weft' }
$version = $env:WEFT_VERSION
$prefix = if ($env:WEFT_PREFIX) { $env:WEFT_PREFIX } else { Join-Path $env:LOCALAPPDATA 'Weft' }
$api = if ($version) { "https://api.github.com/repos/$repository/releases/tags/$version" } else { "https://api.github.com/repos/$repository/releases/latest" }
$release = Invoke-RestMethod -Uri $api -Headers @{ Accept = 'application/vnd.github+json' }
$name = "weft-$($release.tag_name.TrimStart('v'))-x86_64-pc-windows-msvc.tar.gz"
$asset = $release.assets | Where-Object { $_.name -eq $name } | Select-Object -First 1
if (-not $asset -or -not $asset.digest -or -not $asset.digest.StartsWith('sha256:')) { throw "release $($release.tag_name) has no verified asset $name" }
$root = Join-Path ([System.IO.Path]::GetTempPath()) ("weft-install-" + [guid]::NewGuid()); New-Item -ItemType Directory -Force -Path $root | Out-Null
try {
    $archive = Join-Path $root 'runtime.tar.gz'; Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $archive
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
    if ($actual -ne $asset.digest.Substring(7).ToLowerInvariant()) { throw 'GitHub asset digest mismatch' }
    tar -xzf $archive -C $root; $package = Get-ChildItem -LiteralPath $root -Directory -Filter 'weft-*' | Select-Object -First 1
    if (-not $package) { throw 'release archive has no runtime package' }; & (Join-Path $package.FullName 'install.ps1') -Prefix $prefix
} finally { Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue }
