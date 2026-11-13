<#
.SYNOPSIS
    Turns a freshly trained full PGO profile into the runtime-only one the repository ships
    (RustPortPS2/native/pgo/runtime.profdata).

.DESCRIPTION
    install.ps1 -Pgo on (or the launcher's "Optimize now") trains build/pgo/gow.profdata on the game. That profile also covers
    the recompiled EE functions (sub_XXXXXXXX, entry_*), the generated VU1 blocks (vu1n_part*, vu1c_gen*) and templates
    instantiated with microcode words (IL<j|m|i|y><3+ digits>E in the mangled name): all of it derived from the game. This
    script drops every one of them, keeping only this repository's own runtime (GS, VU1 core, audio, IOP, third-party
    libraries), then audits the result: it lists the function names, refuses a profile that still names generated code, and
    refuses one that carries indirect-call value profile data (its hashes could point at generated functions).

    A maintainer runs it after runtime changes and before a release (docs/dev/BUILD.md, "The shipped speed profile").
    Never commit the full profile.

.PARAMETER Full
    The full profile. Default: build/pgo/gow.profdata of the repository build (RustPortPS2/native/build).

.PARAMETER Out
    The runtime-only profile to write. Default: RustPortPS2/native/pgo/runtime.profdata.

.PARAMETER LlvmProfdata
    llvm-profdata. Default: the one on PATH, else the llvm-mingw of the installer (toolchain/ next to the repository's build).
#>
[CmdletBinding()]
param(
    [string]$Full,
    [string]$Out,
    [string]$LlvmProfdata
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Native = Join-Path $Root 'RustPortPS2\native'
if (-not $Full) { $Full = Join-Path $Native 'build\pgo\gow.profdata' }
if (-not $Out) { $Out = Join-Path $Native 'pgo\runtime.profdata' }
if (-not (Test-Path -LiteralPath $Full)) { throw "The full profile is missing: $Full (train it with install.ps1 -Pgo on -RetrainPgo)" }
if (-not $LlvmProfdata) {
    $found = Get-Command llvm-profdata -ErrorAction SilentlyContinue
    if ($found) { $LlvmProfdata = $found.Source }
    else {
        $found = Get-ChildItem -Path (Join-Path $Root 'toolchain') -Filter llvm-profdata.exe -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $LlvmProfdata = $found.FullName } else { throw 'llvm-profdata not found: pass -LlvmProfdata' }
    }
}

# What is derived from the game. Keep in sync with docs/dev/BUILD.md.
$generated = 'sub_[0-9A-F]{8}|entry_[0-9A-Fa-f]+|vu1n_part|vu1c_gen|IL[jmiy][0-9]{3,}E'

$outDir = Split-Path -Parent $Out
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
& $LlvmProfdata merge "--no-function=$generated" -o $Out $Full
if ($LASTEXITCODE -ne 0) { throw 'llvm-profdata merge failed' }

# Audit 1: every function name left must not be generated code.
$all = & $LlvmProfdata show --all-functions $Out
$names = @($all | Where-Object { $_ -match '^  \S.*:$' } | ForEach-Object { $_.Trim().TrimEnd(':') })
$bad = @($names | Where-Object { $_ -match $generated })
if ($bad.Count -gt 0) { throw "The profile still names generated code ($($bad.Count) functions, first: $($bad[0]))" }
if ($names.Count -eq 0) { throw 'The profile holds no functions' }

# Audit 2: no indirect-call value profile (call-site hashes could refer to generated functions).
$sites = @($all | Where-Object { $_ -match 'Indirect Call Site Count: [1-9]' })
if ($sites.Count -gt 0) {
    throw ("The profile carries value profile data ($($sites.Count) functions with indirect call sites). Drop it: " +
        "llvm-profdata merge --text, set each function's indirect call site count to 0 and delete its value-profile lines, then merge the text back with -o.")
}

$size = [math]::Round((Get-Item -LiteralPath $Out).Length / 1MB, 2)
Write-Host "Runtime-only profile written: $Out ($($names.Count) functions, $size MB)"
Write-Host "Kept names checked: none of sub_XXXXXXXX, entry_*, vu1n_part*, vu1c_gen*, templates with microcode words; no indirect-call value profile."
Write-Host 'Commit it (it replaces the previous one) and mention in the message when it was trained and on which commit.'
