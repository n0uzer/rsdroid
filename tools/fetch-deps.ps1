<#
.SYNOPSIS
    Fetch and verify the pinned rsdroid dependencies.

.DESCRIPTION
    Downloads QEMU, scrcpy-server and the Android-x86 image listed in
    tools/deps.json into %LOCALAPPDATA%\rsdroid (or -Dest), verifying every
    SHA-256/SHA-512 from the manifest. Existing files with the right hash are
    reused. No installer is executed: QEMU is downloaded for the user or M1 to
    install.

.PARAMETER Component
    One or more dependency ids from tools/deps.json, or "all" (default).

.PARAMETER Dest
    Data root. Defaults to %LOCALAPPDATA%\rsdroid. Files land under the
    manifest's destination paths, for example tools\scrcpy\scrcpy-server-v5.0.1.

.PARAMETER Force
    Ignore cached files and partial downloads and fetch again.

.PARAMETER CheckOnly
    Do not download. Verify files already present and exit 1 if any is missing
    or does not match its checksum.

.PARAMETER VerifyManifest
    Validate the manifest itself (schema, URLs, checksum format, destinations)
    without touching the network or disk. Used by CI.

.EXAMPLE
    pwsh tools/fetch-deps.ps1

.EXAMPLE
    pwsh tools/fetch-deps.ps1 -Component qemu,scrcpy-server

.EXAMPLE
    pwsh tools/fetch-deps.ps1 -VerifyManifest
#>
[CmdletBinding()]
param(
    [string[]] $Component = @('all'),
    [string] $Dest,
    [switch] $Force,
    [switch] $CheckOnly,
    [switch] $VerifyManifest
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$ManifestPath = Join-Path $PSScriptRoot 'deps.json'

function Write-Step {
    param([string] $Message)
    Write-Host "==> $Message"
}

function Format-Size {
    param([long] $Bytes)
    if ($Bytes -ge 1GB) { return ('{0:N2} GiB' -f ($Bytes / 1GB)) }
    if ($Bytes -ge 1MB) { return ('{0:N1} MiB' -f ($Bytes / 1MB)) }
    if ($Bytes -ge 1KB) { return ('{0:N1} KiB' -f ($Bytes / 1KB)) }
    return "$Bytes B"
}

function Get-HashHex {
    param([string] $Path, [string] $Algorithm)
    return (Get-FileHash -LiteralPath $Path -Algorithm $Algorithm).Hash.ToLowerInvariant()
}

function Test-HashFormat {
    param([string] $Value, [string] $Algorithm)
    if ($Algorithm -eq 'sha256') { return $Value -match '^[0-9a-fA-F]{64}$' }
    if ($Algorithm -eq 'sha512') { return $Value -match '^[0-9a-fA-F]{128}$' }
    return $false
}

function Read-Manifest {
    if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
        throw "manifest not found: $ManifestPath"
    }
    $text = Get-Content -LiteralPath $ManifestPath -Raw -Encoding UTF8
    try {
        $manifest = $text | ConvertFrom-Json
    } catch {
        throw "manifest is not valid JSON: $($_.Exception.Message)"
    }
    if ($null -eq $manifest.schema -or [int] $manifest.schema -ne 1) {
        throw 'manifest schema must be 1'
    }
    if ($null -eq $manifest.deps -or @($manifest.deps).Count -eq 0) {
        throw 'manifest has no deps'
    }

    $ids = @{}
    foreach ($dep in @($manifest.deps)) {
        foreach ($field in 'id', 'version', 'kind', 'url', 'dest', 'license') {
            if ([string]::IsNullOrWhiteSpace([string] $dep.$field)) {
                throw "dependency is missing '$field'"
            }
        }
        if ($ids.ContainsKey([string] $dep.id)) {
            throw "duplicate dependency id: $($dep.id)"
        }
        $ids[[string] $dep.id] = $true

        if ($dep.kind -notin 'installer', 'file', 'image') {
            throw "$($dep.id): unknown kind '$($dep.kind)'"
        }
        if (-not ([string] $dep.url).StartsWith('https://', [StringComparison]::OrdinalIgnoreCase)) {
            throw "$($dep.id): url must use https"
        }
        if ($dep.dest -match '(^[\\/])|(^|/)\.\.(/|$)|(^|/)\.\.$' -or $dep.dest -match ':') {
            throw "$($dep.id): dest must be a relative path without '..'"
        }
        if ($null -eq $dep.hash) {
            throw "$($dep.id): missing hash"
        }
        $algorithm = ([string] $dep.hash.algorithm).ToLowerInvariant()
        if ($algorithm -notin 'sha256', 'sha512') {
            throw "$($dep.id): unsupported hash algorithm '$algorithm'"
        }
        if (-not (Test-HashFormat -Value ([string] $dep.hash.value) -Algorithm $algorithm)) {
            throw "$($dep.id): invalid $algorithm checksum"
        }
        if ($null -ne $dep.size -and [long] $dep.size -le 0) {
            throw "$($dep.id): size must be positive"
        }
    }
    return $manifest
}

function Resolve-Destination {
    param([string] $Root, [string] $Relative)
    $rootFull = [IO.Path]::GetFullPath($Root)
    $full = [IO.Path]::GetFullPath((Join-Path $rootFull ($Relative -replace '/', '\')))
    $prefix = $rootFull.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if (-not $full.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "destination escapes the data root: $Relative"
    }
    return $full
}

function Get-Curl {
    $command = Get-Command 'curl.exe' -ErrorAction SilentlyContinue
    if ($null -ne $command) { return $command.Source }
    $system = Join-Path $env:SystemRoot 'System32\curl.exe'
    if (Test-Path -LiteralPath $system -PathType Leaf) { return $system }
    return $null
}

function Invoke-Download {
    param([string] $Url, [string] $Part)
    $curl = Get-Curl
    if ($null -ne $curl) {
        $arguments = @('--fail', '--location', '--retry', '3', '--retry-delay', '5',
            '--progress-bar', '--output', $Part)
        if (Test-Path -LiteralPath $Part) {
            $arguments += '--continue-at'
            $arguments += '-'
        }
        $arguments += $Url
        & $curl @arguments
        if ($LASTEXITCODE -eq 0) { return }
        if (Test-Path -LiteralPath $Part) { Remove-Item -LiteralPath $Part -Force }
        Write-Warning "curl could not resume; retrying from zero"
        $arguments = @('--fail', '--location', '--retry', '3', '--retry-delay', '5',
            '--progress-bar', '--output', $Part, $Url)
        & $curl @arguments
        if ($LASTEXITCODE -ne 0) { throw "curl failed with exit code $LASTEXITCODE" }
        return
    }
    Write-Warning 'curl.exe not found; falling back to Invoke-WebRequest (no resume)'
    Invoke-WebRequest -Uri $Url -OutFile $Part -UseBasicParsing
}

function Select-Dependencies {
    param($Manifest, [string[]] $Requested)
    $all = @($Manifest.deps)
    if ($Requested -contains 'all') { return $all }
    $known = @{}
    foreach ($dep in $all) { $known[[string] $dep.id] = $dep }
    $selected = @()
    foreach ($id in $Requested) {
        if (-not $known.ContainsKey($id)) {
            throw "unknown component '$id'; known: $(($known.Keys | Sort-Object) -join ', ')"
        }
        $selected += $known[$id]
    }
    return $selected
}

function Test-Dependency {
    param($Dep, [string] $Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return [pscustomobject]@{ Status = 'missing'; Actual = $null }
    }
    $actual = Get-HashHex -Path $Path -Algorithm ([string] $Dep.hash.algorithm)
    if ($actual -eq ([string] $Dep.hash.value).ToLowerInvariant()) {
        return [pscustomobject]@{ Status = 'ok'; Actual = $actual }
    }
    return [pscustomobject]@{ Status = 'mismatch'; Actual = $actual }
}

try {
    $manifest = Read-Manifest
    if ($VerifyManifest) {
        Write-Step "manifest ok: $(@($manifest.deps).Count) dependencies in $ManifestPath"
        foreach ($dep in @($manifest.deps)) {
            Write-Host ("  {0,-14} {1,-10} {2}" -f $dep.id, $dep.version, $dep.hash.algorithm)
        }
        exit 0
    }

    if ([string]::IsNullOrWhiteSpace($Dest)) {
        if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
            throw 'LOCALAPPDATA is not set; pass -Dest'
        }
        $Dest = Join-Path $env:LOCALAPPDATA 'rsdroid'
    }
    $Dest = [IO.Path]::GetFullPath($Dest)
    $selected = @(Select-Dependencies -Manifest $manifest -Requested $Component)

    Write-Step "data root: $Dest"
    if ($CheckOnly) { Write-Step 'check-only mode: nothing will be downloaded' }

    $failures = 0
    $rows = @()
    foreach ($dep in $selected) {
        $path = Resolve-Destination -Root $Dest -Relative ([string] $dep.dest)
        $state = Test-Dependency -Dep $dep -Path $path

        if ($state.Status -eq 'ok' -and -not $Force) {
            Write-Host ("  cached  {0,-14} {1}" -f $dep.id, $path)
            $rows += [pscustomobject]@{ id = $dep.id; status = 'cached'; path = $path }
            continue
        }

        if ($CheckOnly) {
            Write-Host ("  {0,-8}{1,-14} {2}" -f $state.Status, $dep.id, $path)
            $failures++
            $rows += [pscustomobject]@{ id = $dep.id; status = $state.Status; path = $path }
            continue
        }

        if ($Force -and (Test-Path -LiteralPath $path)) {
            Remove-Item -LiteralPath $path -Force
        }
        if ($Force -and (Test-Path -LiteralPath "$path.part")) {
            Remove-Item -LiteralPath "$path.part" -Force
        }

        $directory = Split-Path -Parent $path
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
        $part = "$path.part"
        $started = Get-Date
        Write-Step "downloading $($dep.id) $($dep.version) from $($dep.url)"
        Invoke-Download -Url ([string] $dep.url) -Part $part

        $actual = Get-HashHex -Path $part -Algorithm ([string] $dep.hash.algorithm)
        $expected = ([string] $dep.hash.value).ToLowerInvariant()
        if ($actual -ne $expected) {
            Remove-Item -LiteralPath $part -Force -ErrorAction SilentlyContinue
            throw "$($dep.id): checksum mismatch`n  expected: $expected`n  actual:   $actual"
        }
        Move-Item -LiteralPath $part -Destination $path -Force

        $seconds = [Math]::Round(((Get-Date) - $started).TotalSeconds, 1)
        $size = (Get-Item -LiteralPath $path).Length
        Write-Host ("  fetched {0,-14} {1} in {2}s ({3})" -f $dep.id, $path, $seconds, (Format-Size $size))
        if ($dep.kind -eq 'installer') {
            Write-Host "  note: $($dep.id) is an installer; it was verified but not executed."
        }
        $rows += [pscustomobject]@{ id = $dep.id; status = 'fetched'; path = $path }
    }

    Write-Step 'summary'
    $rows | Format-Table -AutoSize | Out-Host
    if ($failures -gt 0) {
        Write-Error "$failures dependency(ies) missing or invalid; run without -CheckOnly to fetch them"
        exit 1
    }
    Write-Host 'all selected dependencies are present and verified.'
    exit 0
} catch {
    Write-Error $_
    exit 1
}
