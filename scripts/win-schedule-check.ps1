<#
.SYNOPSIS
  Registers the daily scan with the real Task Scheduler, runs it once, and removes it again.

.DESCRIPTION
  Uses a throwaway state folder and a small throwaway scan root. Refuses to run when a task named
  omg\scan already exists, so a real user's task is never replaced or removed.

.EXAMPLE
  powershell -File scripts\win-schedule-check.ps1 -Omg target\debug\omg.exe
#>
param([Parameter(Mandatory)][string]$Omg)
$ErrorActionPreference = 'Stop'
$Omg = (Resolve-Path $Omg).Path

function Status($state) { & $Omg --state-dir $state status | ConvertFrom-Json }
function Registered($state) { (& $Omg --state-dir $state schedule status | ConvertFrom-Json).registered }

& schtasks.exe /Query /TN 'omg\scan' 2>$null | Out-Null
if ($LASTEXITCODE -eq 0) { throw 'a task omg\scan already exists; not touching it' }

$work = Join-Path ([IO.Path]::GetTempPath()) ("omg-sched-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
$root = Join-Path $work 'root'
$state = Join-Path $work 'state'
New-Item -ItemType Directory -Path $root | Out-Null
Set-Content -Path (Join-Path $root 'a.txt') -Value 'a'
try {
    & $Omg --state-dir $state scan --root $root --no-suggest | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'first scan failed' }
    $first = (Status $state).last_report.id

    & $Omg --state-dir $state schedule enable --hour 3
    if ($LASTEXITCODE -ne 0) { throw 'schedule enable failed' }
    if (-not (Registered $state)) { throw 'task not reported as registered' }
    $xml = & schtasks.exe /Query /TN 'omg\scan' /XML
    $command = ($xml | Select-String '<Command>|<Arguments>' | ForEach-Object { $_.Line.Trim() }) -join ' '
    Write-Host "task: $command"
    if ($command -notmatch 'scan --accept-gaps --no-suggest') { throw "unexpected task command: $command" }

    & schtasks.exe /Run /TN 'omg\scan' | Out-Null
    $deadline = (Get-Date).AddSeconds(90)
    do {
        Start-Sleep -Seconds 2
        $report = (Status $state).last_report
    } while ($report.id -eq $first -and (Get-Date) -lt $deadline)
    $info = & schtasks.exe /Query /TN 'omg\scan' /V /FO LIST | Select-String 'Last Run Time|Last Result|Logon Mode'
    $info | ForEach-Object { Write-Host $_.Line }
    if ($report.id -eq $first) { throw 'Task Scheduler did not run the scan within 90 seconds' }
    Write-Host "scan $($report.id) ran from Task Scheduler: $($report.state)"

    & $Omg --state-dir $state schedule disable
    if (Registered $state) { throw 'task still registered after disable' }
    & schtasks.exe /Query /TN 'omg\scan' 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) { throw 'schtasks still lists the task' }
    Write-Host 'PASS: registered, ran once, removed'
} finally {
    & $Omg --state-dir $state schedule disable 2>$null | Out-Null
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
