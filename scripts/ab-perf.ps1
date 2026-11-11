# GOW-Port: A/B frame-rate comparison of two builds of the game (docs/dev/PERFORMANCE.md, "Comparing two builds").
#
#   scripts\ab-perf.ps1 -ExeA <old gow-2005.exe> -ExeB <new gow-2005.exe> -Elf <SCUS_973.99> -EnvFile <gow-cli env output>
#                       [-WorkDir <folder>] [-Pairs 4] [-Seconds 150] [-WarmupSeconds 30] [-PadScript '5:start,12:x,...']
#                       [-Set 'NAME=VALUE', ...] [-SetA 'NAME=VALUE', ...] [-SetB 'NAME=VALUE', ...] [-OutDir <folder>]
#                       [-Cores 4]
#
# -Cores N pins the game to the first N logical processors, to approximate a weaker PC (the target of most performance
# work); 0 (the default) leaves the affinity alone.
#
# -SetA / -SetB apply to one variant only: with the same executable as A and B they compare a switch on and off (for example
# -SetA 'GOW_FRAME_DIAG=0' to measure what the diagnostics cost).
#
# It alternates the two executables (A B, B A, A B, ...) with the same environment, the same disc and the same pad script,
# reads the [gow-perf] and [gow-frames] lines of each run and prints, per variant, the guest frame rate (mean and worst 5 s
# window) and the frame-time percentiles. Only alternating pairs count: run-to-run noise is several fps.
#
# The environment is the launcher's own: write it once with `cargo run -p gow-cli -- env > env.txt` from gow-launcher/
# (NAME=VALUE lines; -NAME removes a variable). GOW_PERF_DIAG=1, GOW_FRAME_DIAG=1 and, with -PadScript, GOW_PAD_TEST=1 and
# GOW_PAD_SCRIPT are added on top. -WorkDir is the folder the game starts in (the launcher uses its data folder).
#
# Rules (AGENTS.md, "Rules for living with the user's game"): the script refuses to start while a gow-2005 is running, never
# closes a process by name, and closes only the processes it started (by PID), first through their window. Nothing else should
# use the CPU or GPU while it runs: a build or another game makes the numbers meaningless. Ask the player before running it.
param(
    [Parameter(Mandatory = $true)][string]$ExeA,
    [Parameter(Mandatory = $true)][string]$ExeB,
    [Parameter(Mandatory = $true)][string]$Elf,
    [Parameter(Mandatory = $true)][string]$EnvFile,
    [string]$WorkDir = '',
    [int]$Pairs = 4,
    [int]$Seconds = 150,
    [int]$WarmupSeconds = 30,
    [string]$PadScript = '',
    [string[]]$Set = @(),
    [string[]]$SetA = @(),
    [string[]]$SetB = @(),
    [string]$OutDir = '',
    [int]$Cores = 0
)
$ErrorActionPreference = 'Stop'

function Resolve-Existing([string]$path, [string]$what) {
    if (-not (Test-Path -LiteralPath $path)) { throw "$what not found: $path" }
    return (Resolve-Path -LiteralPath $path).Path
}

$ExeA = Resolve-Existing $ExeA 'ExeA'
$ExeB = Resolve-Existing $ExeB 'ExeB'
$Elf = Resolve-Existing $Elf 'Elf'
$EnvFile = Resolve-Existing $EnvFile 'EnvFile'
if (-not $WorkDir) { $WorkDir = Split-Path -Parent $Elf }
$WorkDir = Resolve-Existing $WorkDir 'WorkDir'
if (-not $OutDir) { $OutDir = Join-Path (Split-Path -Parent $PSScriptRoot) ('logs\ab-perf\' + (Get-Date -Format 'yyyyMMdd-HHmmss')) }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
if ($Pairs -lt 1) { throw '-Pairs must be at least 1.' }

# Only a check: nothing here stops a process that this script did not start.
$running = @(Get-Process -Name 'gow-2005' -ErrorAction SilentlyContinue)
if ($running.Count -gt 0) {
    throw "A gow-2005 is already running (PID $($running.Id -join ', ')). Close it first; this script never closes it."
}

# The launcher's environment, then the measurement switches on top.
$envSet = [ordered]@{}
$envRemove = New-Object System.Collections.Generic.List[string]
foreach ($line in Get-Content -LiteralPath $EnvFile) {
    $text = $line.Trim()
    if (-not $text) { continue }
    if ($text.StartsWith('-')) { $envRemove.Add($text.Substring(1)); continue }
    $at = $text.IndexOf('=')
    if ($at -gt 0) { $envSet[$text.Substring(0, $at)] = $text.Substring($at + 1) }
}
if (-not $envSet.Contains('PS2X_GS_WORKER') -or $envSet['PS2X_GS_WORKER'] -ne '1') {
    Write-Warning 'PS2X_GS_WORKER=1 is missing from the environment file: players never run without it.'
}
$envSet['GOW_PERF_DIAG'] = '1'
$envSet['GOW_FRAME_DIAG'] = '1'
if ($PadScript) {
    $envSet['GOW_PAD_TEST'] = '1'
    $envSet['GOW_PAD_SCRIPT'] = $PadScript
    # The pad test saves a screenshot every 10-20 s by default (gow_pad_test.cpp): each one is a 160-190 ms frame that
    # players never see and that counted as a hitch. -Set 'GOW_PAD_TEST_NO_CAPTURE=0' brings them back.
    $envSet['GOW_PAD_TEST_NO_CAPTURE'] = '1'
}
function ConvertTo-Pairs([string[]]$items, [string]$parameter) {
    $pairs = [ordered]@{}
    foreach ($item in $items) {
        $at = $item.IndexOf('=')
        if ($at -le 0) { throw "$parameter expects NAME=VALUE, got '$item'" }
        $pairs[$item.Substring(0, $at)] = $item.Substring($at + 1)
    }
    return $pairs
}
$common = ConvertTo-Pairs $Set '-Set'
foreach ($name in $common.Keys) { $envSet[$name] = $common[$name] }
$variantEnv = @{ A = (ConvertTo-Pairs $SetA '-SetA'); B = (ConvertTo-Pairs $SetB '-SetB') }

# The project never runs above 120 FPS (AGENTS.md): no uncapped runs. They kept the GPU at 100 % until the AMD OpenGL driver
# crashed, and they measure nothing a player sees.
foreach ($capped in @($envSet, $variantEnv['A'], $variantEnv['B'])) {
    # (not $pairs: PowerShell names are case-insensitive, and $Pairs is the [int] parameter)
    foreach ($name in @('PC_LOGIC_FPS', 'PC_WINDOW_FPS')) {
        if (-not $capped.Contains($name)) { continue }
        $value = 0
        if (-not [int]::TryParse([string]$capped[$name], [ref]$value) -or $value -gt 120 -or ($name -eq 'PC_WINDOW_FPS' -and $value -le 0)) {
            throw "$name=$($capped[$name]) is above the 120 FPS cap (or unlimited): the project never runs above 120 FPS"
        }
    }
}

# Start-Process in Windows PowerShell 5.1 takes no environment block: the child inherits this process's, so it is set here
# for the run and restored afterwards.
function Invoke-Run([string]$label, [string]$exe, [int]$index) {
    $log = Join-Path $OutDir ('{0:00}-{1}.log' -f $index, $label)
    $extra = $variantEnv[$label]
    $saved = @{}
    foreach ($name in @($envSet.Keys) + @($envRemove) + @($extra.Keys)) { $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
    try {
        foreach ($name in $envRemove) { [Environment]::SetEnvironmentVariable($name, $null, 'Process') }
        foreach ($name in $envSet.Keys) { [Environment]::SetEnvironmentVariable($name, $envSet[$name], 'Process') }
        foreach ($name in $extra.Keys) { [Environment]::SetEnvironmentVariable($name, $extra[$name], 'Process') }
        Write-Host ("[{0}] run {1} ({2}): {3} s" -f (Get-Date -Format 'HH:mm:ss'), $index, $label, $Seconds)
        $process = Start-Process -FilePath $exe -ArgumentList ('"' + $Elf + '"') -WorkingDirectory $WorkDir -PassThru `
            -RedirectStandardError $log -RedirectStandardOutput ($log + '.out')
        if ($Cores -gt 0) { $process.ProcessorAffinity = [IntPtr]([int64](1 -shl $Cores) - 1) }
    }
    finally {
        foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
    }
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline -and -not $process.HasExited) { Start-Sleep -Milliseconds 500 }
    $exitedEarly = $process.HasExited
    if (-not $process.HasExited) {
        # Only this run's own process, by its PID: first a normal close through its window, then a forced stop.
        [void]$process.CloseMainWindow()
        if (-not $process.WaitForExit(15000)) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    }
    return [pscustomobject]@{ Label = $label; Index = $index; Log = $log; Pid = $process.Id; ExitedEarly = $exitedEarly }
}

function Read-Number([string]$line, [string]$key) {
    $m = [regex]::Match($line, '(?:^|\s)' + [regex]::Escape($key) + '=([-0-9.]+)')
    if ($m.Success) { return [double]::Parse($m.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture) }
    return $null
}

function Measure-Log($run) {
    $lines = Get-Content -LiteralPath $run.Log -ErrorAction SilentlyContinue
    $fps = New-Object System.Collections.Generic.List[double]
    $vu = New-Object System.Collections.Generic.List[double]
    $ee = New-Object System.Collections.Generic.List[double]
    $p95 = New-Object System.Collections.Generic.List[double]
    $p99 = New-Object System.Collections.Generic.List[double]
    $over = 0.0
    $over50 = 0.0
    $worker = $false
    foreach ($line in $lines) {
        if ($line -match '\[gs-worker\] VIF1/GIF processing on a worker thread') { $worker = $true }
        $t = Read-Number $line 't'
        if ($null -eq $t -or $t -lt $WarmupSeconds) { continue }
        if ($line.StartsWith('[gow-perf]')) {
            $hz = Read-Number $line 'guest_flip_hz'
            if ($null -ne $hz -and $hz -gt 0) { $fps.Add($hz) }
            $v = Read-Number $line 'vu_ms'; if ($null -ne $v) { $vu.Add($v) }
            $e = Read-Number $line 'ee_ms'; if ($null -ne $e) { $ee.Add($e) }
        }
        elseif ($line.StartsWith('[gow-frames]')) {
            $a = Read-Number $line 'p95_ms'; if ($null -ne $a) { $p95.Add($a) }
            $b = Read-Number $line 'p99_ms'; if ($null -ne $b) { $p99.Add($b) }
            $o = Read-Number $line 'over16_7ms'; if ($null -ne $o) { $over += $o }
            $h = Read-Number $line '>50ms'; if ($null -ne $h) { $over50 += $h }
        }
    }
    $mean = { param($list) if ($list.Count) { ($list | Measure-Object -Average).Average } else { [double]::NaN } }
    return [pscustomobject]@{
        Label = $run.Label; Index = $run.Index; Pid = $run.Pid; ExitedEarly = $run.ExitedEarly; GsWorker = $worker
        Windows = $fps.Count
        FpsMean = & $mean $fps
        FpsWorst = if ($fps.Count) { ($fps | Measure-Object -Minimum).Minimum } else { [double]::NaN }
        P95Mean = & $mean $p95
        P99Worst = if ($p99.Count) { ($p99 | Measure-Object -Maximum).Maximum } else { [double]::NaN }
        FramesOver16 = $over
        FramesOver50 = $over50
        VuMsMean = & $mean $vu
        EeMsMean = & $mean $ee
        Log = $run.Log
    }
}

Write-Host "A: $ExeA $(($variantEnv.A.Keys | ForEach-Object { "$_=$($variantEnv.A[$_])" }) -join ' ')"
Write-Host "B: $ExeB $(($variantEnv.B.Keys | ForEach-Object { "$_=$($variantEnv.B[$_])" }) -join ' ')"
Write-Host "Logs: $OutDir"
$results = New-Object System.Collections.Generic.List[object]
$index = 0
for ($pair = 0; $pair -lt $Pairs; $pair++) {
    # A B, then B A, so neither variant always runs on a cooler or warmer machine.
    $order = if ($pair % 2 -eq 0) { @('A', 'B') } else { @('B', 'A') }
    foreach ($label in $order) {
        $index++
        $exe = if ($label -eq 'A') { $ExeA } else { $ExeB }
        $result = Measure-Log (Invoke-Run $label $exe $index)
        $results.Add($result)
        if (-not $result.GsWorker) { Write-Warning "run $index ($label): no '[gs-worker] VIF1/GIF processing on a worker thread' line in its log" }
        if ($result.ExitedEarly) { Write-Warning "run $index ($label) exited before $Seconds s; see $($result.Log)" }
        Write-Host ("  fps mean {0:N2} worst {1:N2}  p95 {2:N2} ms  p99 max {3:N2} ms  vu_ms {4:N0}" -f $result.FpsMean, $result.FpsWorst, $result.P95Mean, $result.P99Worst, $result.VuMsMean)
    }
}

$results | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $OutDir 'results.json') -Encoding ASCII
Write-Host ''
Write-Host 'Per variant (mean of its runs):'
foreach ($label in @('A', 'B')) {
    $runs = @($results | Where-Object { $_.Label -eq $label })
    $avg = { param($name) ($runs | ForEach-Object { $_.$name } | Where-Object { -not [double]::IsNaN($_) } | Measure-Object -Average).Average }
    Write-Host ("  {0}: fps {1:N2}  worst window {2:N2}  p95 {3:N2} ms  frames over 16.7 ms {4:N0}  over 50 ms {5:N1}  ({6} runs)" -f $label, (& $avg 'FpsMean'), (& $avg 'FpsWorst'), (& $avg 'P95Mean'), (& $avg 'FramesOver16'), (& $avg 'FramesOver50'), $runs.Count)
}
$wins = 0
for ($pair = 0; $pair -lt $Pairs; $pair++) {
    $a = $results | Where-Object { $_.Label -eq 'A' } | Select-Object -Skip $pair -First 1
    $b = $results | Where-Object { $_.Label -eq 'B' } | Select-Object -Skip $pair -First 1
    if ($a -and $b -and $b.FpsMean -gt $a.FpsMean) { $wins++ }
}
Write-Host ("  B faster than A in {0} of {1} pairs (mean fps)" -f $wins, $Pairs)
Write-Host "Details: $(Join-Path $OutDir 'results.json')"
