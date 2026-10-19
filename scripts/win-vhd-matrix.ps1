<#
.SYNOPSIS
  Scans real NTFS, FAT32, exFAT and ReFS volumes made from throwaway virtual disks (release plan R13).

.DESCRIPTION
  Each virtual disk is created, formatted, mounted, filled with a few known files, scanned with omg
  from the volume root, checked against an independent count, then detached and deleted. omg itself
  never elevates; only this script needs an elevated PowerShell, because diskpart attaches the disks.
  Nothing outside the temporary folder and the virtual disks is touched.

.EXAMPLE
  powershell -File scripts\win-vhd-matrix.ps1 -Omg target\debug\omg.exe
  powershell -File scripts\win-vhd-matrix.ps1 -Omg target\debug\omg.exe -Optional ReFS
#>
param(
    [Parameter(Mandatory)][string]$Omg,
    [string[]]$FileSystems = @('NTFS', 'FAT32', 'exFAT', 'ReFS'),
    # Filesystems that may be skipped when Windows cannot format them (ReFS on Home editions).
    [string[]]$Optional = @()
)
$ErrorActionPreference = 'Stop'
$Omg = (Resolve-Path $Omg).Path
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this from an elevated PowerShell: diskpart needs it to attach virtual disks.'
}

$work = Join-Path ([IO.Path]::GetTempPath()) ("omg-vhd-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null

function Invoke-DiskPart([string]$Commands) {
    $file = Join-Path $work 'diskpart.txt'
    Set-Content -Path $file -Value $Commands -Encoding ascii
    $output = (& diskpart /s $file 2>&1 | Out-String)
    # diskpart exits 0 even when a command fails.
    if ($output -match 'DiskPart has encountered an error|Virtual Disk Service error') {
        throw "diskpart failed:`n$output"
    }
    return $output
}

function Test-Volume([string]$FileSystem) {
    $vhd = Join-Path $work "$FileSystem.vhdx"
    $label = "OMG$($FileSystem.ToUpper())"
    $size = if ($FileSystem -eq 'ReFS') { 2048 } else { 512 }   # MB; ReFS needs a larger volume
    $attached = $false
    try {
        Invoke-DiskPart @"
create vdisk file="$vhd" maximum=$size type=expandable
attach vdisk
create partition primary
format fs=$($FileSystem.ToLower()) quick label=$label
assign
"@ | Out-Null
        $attached = $true
        $volume = Get-Volume | Where-Object { $_.FileSystemLabel -eq $label } | Select-Object -First 1
        if (-not $volume -or -not $volume.DriveLetter) { throw "no drive letter for $label" }
        $root = "$($volume.DriveLetter):\"

        # A known tree: nested folders, an empty file, a larger file and (where supported) a hard link.
        New-Item -ItemType Directory -Path "$root\docs\nested" | Out-Null
        Set-Content -Path "$root\docs\a.txt" -Value 'hello' -NoNewline
        New-Item -ItemType File -Path "$root\docs\nested\empty.dat" | Out-Null
        [IO.File]::WriteAllBytes("$root\big.bin", (New-Object byte[] 200000))
        if ($FileSystem -in 'NTFS', 'ReFS') {
            cmd /c "mklink /H `"$root\docs\a-link.txt`" `"$root\docs\a.txt`"" 2>&1 | Out-Null
        }
        $expected = (Get-ChildItem -Path $root -Recurse -File -Force |
            Where-Object { $_.FullName -notmatch 'System Volume Information|\$RECYCLE' } |
            Measure-Object -Property Length -Sum)

        $state = Join-Path $work "state-$FileSystem"
        $scan = (& $Omg --state-dir $state scan --root $root 2>&1 | Out-String)
        if ($LASTEXITCODE -ne 0) { throw "scan failed:`n$scan" }
        $report = $scan | ConvertFrom-Json
        if ($report.state -ne 'complete' -or $report.errors -ne 0) { throw "scan was not clean:`n$scan" }
        $status = (& $Omg --state-dir $state status 2>&1 | Out-String) | ConvertFrom-Json
        # The file bytes the scan indexes must equal an independent count, with a hard-linked file
        # counted once (the oracle lists both names, so it is allowed to be larger by that file).
        $linked = if ($FileSystem -in 'NTFS', 'ReFS' -and (Test-Path "$root\docs\a-link.txt")) { 5 } else { 0 }
        $want = $expected.Sum - $linked
        if ($status.unique_apparent -ne $want) {
            throw "indexed $($status.unique_apparent) bytes, expected $want"
        }
        $doctor = (& $Omg --state-dir $state doctor 2>&1 | Out-String)
        if ($doctor -notmatch 'permanent delete: unsupported|permanent delete: development-unqualified|permanent delete: ') {
            throw "doctor output not understood:`n$doctor"
        }
        [pscustomobject]@{ FileSystem = $FileSystem; Result = 'ok'; Entries = $report.entries; Bytes = $status.unique_apparent }
    }
    finally {
        if ($attached) {
            try { Invoke-DiskPart "select vdisk file=`"$vhd`"`ndetach vdisk" | Out-Null } catch { Write-Warning "detach failed: $_" }
        }
    }
}

$results = @()
$failed = $false
try {
    foreach ($fs in $FileSystems) {
        try {
            $results += Test-Volume $fs
        }
        catch {
            if ($Optional -contains $fs) {
                $results += [pscustomobject]@{ FileSystem = $fs; Result = "skipped: $($_.Exception.Message.Split("`n")[0])"; Entries = ''; Bytes = '' }
            }
            else {
                $results += [pscustomobject]@{ FileSystem = $fs; Result = "FAILED: $($_.Exception.Message)"; Entries = ''; Bytes = '' }
                $failed = $true
            }
        }
    }
}
finally {
    Remove-Item -Recurse -Force -Path $work -ErrorAction SilentlyContinue
}
$results | Format-List
if ($failed) { exit 1 }
