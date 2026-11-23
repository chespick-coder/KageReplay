param(
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$installer = $PSScriptRoot
$generated = Join-Path $installer 'build'
Set-Location $root

if (-not $SkipBuild) {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    cargo test --release
    if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }
}

$exe = Join-Path $root 'target\release\KageReplay.exe'
if (-not (Test-Path $exe)) { throw "not found: $exe" }

$manifest = Get-Content -Path (Join-Path $root 'Cargo.toml') -Encoding utf8
$version = ($manifest | Select-String -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$exeVersion = (Get-Item $exe).VersionInfo.ProductVersion
if ($exeVersion -ne $version) { throw "KageReplay.exe reports version '$exeVersion', Cargo.toml says '$version'" }

$icon = Get-ChildItem -Path (Join-Path $root 'target\release\build') -Filter 'kage.ico' -Recurse |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $icon) { throw 'kage.ico not found, run cargo build first' }

New-Item -ItemType Directory -Force -Path $generated | Out-Null
Copy-Item $icon.FullName (Join-Path $generated 'kage.ico') -Force

Add-Type -AssemblyName System.Drawing
$source = New-Object System.Drawing.Icon((Join-Path $generated 'kage.ico'), 256, 256)
$sourceBitmap = $source.ToBitmap()
foreach ($size in 58, 116) {
    $bitmap = New-Object System.Drawing.Bitmap($size, $size)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $graphics.DrawImage($sourceBitmap, 0, 0, $size, $size)
    $bitmap.Save((Join-Path $generated "small-$size.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose()
    $bitmap.Dispose()
}
$sourceBitmap.Dispose()
$source.Dispose()

$tools = @(
    (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6'),
    (Join-Path $env:ProgramFiles 'Inno Setup 6'),
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6')
) | Where-Object { Test-Path (Join-Path $_ 'ISCC.exe') } | Select-Object -First 1
if (-not $tools) { throw 'Inno Setup 6 not found: winget install JRSoftware.InnoSetup' }

$key = Join-Path $installer 'signing\release.key'
if (-not (Test-Path $key)) { throw "signing key not found: $key" }

$release = Join-Path $root 'dist\release'
New-Item -ItemType Directory -Force -Path $release | Out-Null
$published = Join-Path $release 'KageReplay.exe'
Copy-Item $exe $published -Force
& (Join-Path $tools 'ISSigTool.exe') "--key-file=$key" --allow-overwrite --quiet sign $published
if ($LASTEXITCODE -ne 0) { throw 'ISSigTool sign failed' }
& (Join-Path $tools 'ISSigTool.exe') "--key-file=$key" --quiet verify $published
if ($LASTEXITCODE -ne 0) { throw 'ISSigTool verify failed' }

& (Join-Path $tools 'ISCC.exe') /Q (Join-Path $installer 'KageReplay.iss')
if ($LASTEXITCODE -ne 0) { throw 'ISCC failed' }

Get-ChildItem $release | ForEach-Object { Write-Output ("{0}  {1:N1} MB" -f $_.Name, ($_.Length / 1MB)) }
Write-Output "version $version ready in $release"
