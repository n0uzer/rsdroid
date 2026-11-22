<#
.SYNOPSIS
    Extract the Android-x86 kernel and initrd needed by `rsdroid run`.

.DESCRIPTION
    M1 boots the pinned Android-x86 ISO live. QEMU needs the kernel and initrd as
    files so rsdroid can add `VIRT_WIFI=0` to the kernel command line; without it
    Android renames eth0 to wifi_eth and ADB never gets a usable network.

    The ISO stays the verified source of truth. This script only extracts two
    files into the image cache and never modifies the ISO.

.PARAMETER Iso
    Android-x86 ISO. Defaults to
    %LOCALAPPDATA%\rsdroid\images\android-x86_64-9.0-r2.iso.

.PARAMETER OutDir
    Destination for kernel and initrd.img. Defaults to
    %LOCALAPPDATA%\rsdroid\images\android-x86_64-9.0-r2.

.PARAMETER Force
    Overwrite existing extracted files.

.EXAMPLE
    pwsh tools/extract-boot-files.ps1
#>
[CmdletBinding()]
param(
    [string]$Iso,
    [string]$OutDir,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

function Get-DataRoot {
    if ($env:RSDROID_DATA_DIR) { return $env:RSDROID_DATA_DIR }
    if ($env:LOCALAPPDATA) { return (Join-Path $env:LOCALAPPDATA 'rsdroid') }
    throw 'LOCALAPPDATA is not set; pass -Iso and -OutDir explicitly.'
}

function Find-SevenZip {
    foreach ($name in '7z', '7zz', '7za') {
        $command = Get-Command $name -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($command) { return $command.Source }
    }
    foreach ($path in @(
        'C:\Program Files\7-Zip\7z.exe',
        'C:\Program Files (x86)\7-Zip\7z.exe',
        (Join-Path $env:LOCALAPPDATA 'Programs\7-Zip\7z.exe')
    )) {
        if ($path -and (Test-Path -LiteralPath $path -PathType Leaf)) { return $path }
    }
    return $null
}

$dataRoot = Get-DataRoot
if (-not $Iso) {
    $Iso = Join-Path $dataRoot 'images\android-x86_64-9.0-r2.iso'
}
if (-not $OutDir) {
    $OutDir = Join-Path $dataRoot 'images\android-x86_64-9.0-r2'
}

if (-not (Test-Path -LiteralPath $Iso -PathType Leaf)) {
    throw "ISO not found: $Iso. Run 'pwsh tools/fetch-deps.ps1 -Component android-x86' first."
}

$sevenZip = Find-SevenZip
if (-not $sevenZip) {
    throw "7-Zip or NanaZip was not found. Install 7-Zip, or extract 'kernel' and 'initrd.img' from $Iso into $OutDir manually."
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Write-Host "==> ISO: $Iso"
Write-Host "==> destination: $OutDir"

foreach ($name in 'kernel', 'initrd.img') {
    $destination = Join-Path $OutDir $name
    if ((Test-Path -LiteralPath $destination -PathType Leaf) -and -not $Force) {
        Write-Host "  cached  $name  $destination"
        continue
    }
    & $sevenZip e $Iso "-o$OutDir" $name -y | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "7-Zip failed to extract $name (exit $LASTEXITCODE)."
    }
    if (-not (Test-Path -LiteralPath $destination -PathType Leaf)) {
        throw "Extraction did not produce $destination."
    }
    $size = (Get-Item -LiteralPath $destination).Length
    if ($size -le 0) {
        throw "Extracted $name is empty."
    }
    Write-Host "  wrote   $name  $destination ($size bytes)"
}

Write-Host '==> boot files ready'
