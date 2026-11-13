# PC Port - Windows installer (PowerShell 5.1 or 7).
#
#   install.cmd [-Iso "C:\path\God of War.iso" (or .chd)] [-Destination "D:\GodOfWar"] [-InstallTools] [-NoOptimize] [-Optimize] [-Seconds 240]
#
#   -Destination   folder for the extracted disc, the generated code and the build (by default, the repository).
#
#   -Jobs N        build processes (default: logical processors minus a reserve of 2, 1 or 0, at most one per 2 GB of RAM).
#   -NormalPriority  keep the build at normal priority (by default the installer lowers it so the PC stays usable).
#   -SkipCpuCheck  do not stop on a processor without SSE4.1 (the game needs it; the check can be wrong inside a virtual machine).
#
#   -RecompExe     a built ps2_recomp.exe (the Rust recompiler, RustPortPS2/crates/ps2-recomp-cli). Without it the script uses
#                  bin\ps2_recomp.exe next to itself (the launcher's kit carries it) and otherwise builds it with cargo (needs
#                  Rust, a C compiler and the MSVC environment this script already loads).
#   -ToolsExe      same for ps2-tools.exe (RustPortPS2/crates/ps2-tools: ISO extraction, ELF check, VU1/IOP generators).
#
#   -InstallTools  first installs Visual Studio 2022 Build Tools (C++, CMake and Ninja) if missing: downloads the official installer
#                  from Microsoft and asks for administrator permission. Python is not used any more; Git is not needed either:
#                  if it is missing, a portable one is downloaded into <destination>\toolchain.
#
# Steps (each one is skipped if already done):
#   1. Extracts the disc from the ISO into game\ (with its sector map, .disc_layout) and prepares game\IOP_MOD.
#   2. Builds the Rust tools (ps2_recomp, ps2-tools) and translates SCUS_973.99 to C++ with RustPortPS2\games\scus-97399\gow-port.toml.
#   3. Builds the game (gow-2005.exe, MSVC + Ninja). The launcher is a separate download (GitHub Releases): its
#      setup wizard runs this script and finds the result.
#      Before the build the VU1 microcode and the native IOP modules are created from the ELF and the disc's IRX files
#      (vu1.manifest, iop.manifest in games\scus-97399), without playing.
#   4. Optimization: runs the game for a few minutes with automatic key presses, saves the VU1 microcode and the IOP
#      modules it uses and rebuilds with them as native code. Only the first time; -Optimize repeats it,
#      -NoOptimize skips it.
#
# Nothing generated from the game (disc, C++ code, microcode) leaves this computer.
param(
    [string]$Iso = '',
    [switch]$InstallTools,
    [switch]$NoOptimize,
    [switch]$Optimize,
    [int]$Seconds = 240,
    [int]$Jobs = 0,
    # GOW-Port: the installer lowers its own priority so the PC stays usable while the game builds; this keeps it normal.
    [switch]$NormalPriority,
    # GOW-Port: skips the processor check (SSE4.1), for virtualization that hides a feature the CPU really has.
    [switch]$SkipCpuCheck,
    # GOW-Port: protects normal PCs from the VU1 AOT generator retaining every capture in a long session.
    # Zero preserves an explicit developer override; the installer and launcher default to 4 GB.
    [ValidateRange(0, 64)]
    [int]$Vu1MemoryLimitGB = 4,
    # GOW-Port: automated execution (the launcher's installation wizard).
    [switch]$NoPause,
    [switch]$AllowModifiedElf,
    # GOW-Port: development iteration: RelWithDebInfo instead of Release, runtime logs and the console window on, and the VU1 AOT
    # set frozen in a snapshot. The normal Release has no whole-program optimization either (PS2X_ENABLE_LTCG is opt-in and can take
    # hours to link).
    [switch]$FastDev,
    [string]$Vu1BuildDir = '',
    # GOW-Port: folder where everything generated from the game goes (game\, build-gen\, build\, vu1_captures\).
    # Empty: inside the repository, as always. Used by the launcher's installation wizard.
    [string]$Destination = '',
    # GOW-Port: a prebuilt ps2_recomp to use instead of the kit's or the cargo build (see the header).
    [string]$RecompExe = '',
    # GOW-Port: a prebuilt ps2-tools, same rule as -RecompExe.
    [string]$ToolsExe = '',
    # GOW-Port: the C++ compiler of the game. clang (default): llvm-mingw downloaded into toolchain\ with a pinned SHA-256,
    # nothing installed on Windows and no Visual Studio (decision D1 of the 0.3.0 plan; most install failures came from Visual
    # Studio: issues #9, #14, #19, #21, #23). msvc: Visual Studio Build Tools, as before.
    [ValidateSet('clang', 'msvc')]
    [string]$Compiler = 'clang',
    # GOW-Port: profile-guided optimization (clang only). auto (default): the installation and "Recompilar agora" build once,
    # with the profile this PC trained if there is one, else with the runtime-only profile shipped in the repository
    # (RustPortPS2/native/pgo/runtime.profdata: our own runtime, nothing from the game), so the game never opens by itself
    # during an installation and the player can play after a single build. on: also trains the full profile when there is none
    # (the launcher's "Optimize now", run with the game closed: build with instrumentation, the optimization run, build with the
    # profile; the same run gives the native IOP modules). off: none at all. -RetrainPgo trains again although a profile exists
    # (an existing one is reused after an update: functions that changed are just compiled without profile data).
    [ValidateSet('auto', 'on', 'off')]
    [string]$Pgo = 'auto',
    [switch]$RetrainPgo
)
$ErrorActionPreference = 'Stop'
$Root = $PSScriptRoot
$Native = Join-Path $Root 'RustPortPS2\native'
$Profile = Join-Path $Root 'RustPortPS2\games\scus-97399' # the game's recompiler configuration, function map and VU1 data
$DataRoot = $Root
if ($Destination) {
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    $DataRoot = (Resolve-Path -LiteralPath $Destination).Path
}
# GOW-Port: the build writes deep paths (build\ps2xRuntime\CMakeFiles\...). A long folder, one with spaces or one with accents made steps
# fail with path errors (issue #15: it worked after moving to a short folder). Only a warning: it does not fail for everyone.
if ($DataRoot -match '\s' -or $DataRoot -match '[^\x20-\x7E]' -or $DataRoot.Length -gt 60) {
    Write-Host "[warning] The folder $DataRoot has spaces, accents or a long path. If a step fails with a path error, install to a short folder without spaces, for example D:\GoW."
}
$Build = if ($Destination) { Join-Path $DataRoot 'build' } else { Join-Path $Native $(if ($FastDev) { 'build-dev' } else { 'build' }) }
$Game = Join-Path $DataRoot 'game'
$Vu1 = Join-Path $DataRoot 'vu1_captures'
# GOW-Port: freeze the development AOT set; new captures do not invalidate the executable on every test.
$snapshotVu1 = Join-Path $DataRoot 'build-gen\vu1-snapshot'
$Vu1ForBuild = if ($Vu1BuildDir) { $Vu1BuildDir } elseif ($FastDev -and (Test-Path $snapshotVu1)) { $snapshotVu1 } else { $Vu1 }
if ($Vu1BuildDir -and -not (Test-Path -LiteralPath $Vu1BuildDir -PathType Container)) {
    throw "The VU1 directory does not exist: $Vu1BuildDir"
}
if ($Vu1BuildDir) { $Vu1ForBuild = (Resolve-Path -LiteralPath $Vu1BuildDir).Path }
# GOW-Port: build processes: the logical processors minus a reserve that keeps the PC responsive (2 from 8 processors, 1 from 4,
# none below that), and a memory bound: one compiler per GB above 3 GB (the system and the VU1 generator, which may run next to
# the compilers, keep those), or one per 2 GB if that gives more, so no PC gets fewer than with the former rule.
# Measured on the game build (MSVC /O2, unity batches of 8, 12 threads / 32 GB): about 0.35 GB peak per cl.exe, under 1 GB for
# four together; the former one per 2 GB, an estimate from the /GL builds, held back PCs with 8-16 GB.
# Always at least one. -Jobs N sets it and skips the reserve. A function of its two inputs so that it can be tested without WMI
# (scripts/ci/test-install-ps1.ps1).
function Get-BuildJobs([int]$Cores, [int]$MemGb) {
    $reserve = if ($Cores -ge 8) { 2 } elseif ($Cores -ge 4) { 1 } else { 0 }
    $byMemory = [math]::Max([math]::Floor($MemGb / 2), $MemGb - 3)
    return [int][math]::Max(1, [math]::Min($Cores - $reserve, $byMemory))
}
if ($Jobs -le 0) {
    $memGb = [math]::Floor((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB)
    $Jobs = Get-BuildJobs ([Environment]::ProcessorCount) $memGb
}
# GOW-Port: below-normal priority from the start: cargo, cmake, ninja, cl, link and ps2-tools inherit it, so a long build does not
# freeze the player's PC. -NormalPriority keeps it normal. The optimization run of the game resets its own (stage 4).
if (-not $NormalPriority) {
    try {
        [Diagnostics.Process]::GetCurrentProcess().PriorityClass = [Diagnostics.ProcessPriorityClass]::BelowNormal
        Write-Host 'Build priority: below normal (use -NormalPriority to keep it normal)'
    }
    catch { Write-Host "Build priority: could not be lowered ($($_.Exception.Message))" }
}
function Step([string]$t) { Write-Host ''; Write-Host "==> $t" -ForegroundColor Red }
function Warn([string]$t) { Write-Host "[warning] $t" -ForegroundColor Yellow }
function Fail([string]$t) {
    Write-Host "[error] $t" -ForegroundColor Red
    if (-not $NoPause) { Read-Host 'Press Enter to exit' }
    exit 1
}
function Run([string]$exe, [string[]]$arguments) {
    & $exe @arguments
    if ($LASTEXITCODE -ne 0) { Fail "$exe ended with code $LASTEXITCODE" }
}
function Slashes([string]$p) { return ($p -replace '\\', '/') }

# GOW-Port: the processor must have SSE4.1, and nothing newer is needed: the runtime uses SSE4.1 intrinsics throughout (extract,
# blendv, insert, min/max, mullo, round, testz) and none of POPCNT, SSE4.2, AVX or BMI. A machine without it would build for an hour
# and then crash with an illegal instruction. Keep this in step with the launcher's check (gow-core installer.rs) and with
# PS2X_VU1_AVX2: if the installer ever turns that on, AVX2 becomes a requirement too.
$CpuRequirement = 'SSE4.1'
# $hasSse41 is $true or $false from the processor feature query, or $null when it could not be asked (then nothing is blocked).
function Get-CpuProblem($hasSse41) {
    if ($hasSse41 -eq $false) {
        return "This PC's processor does not support $CpuRequirement, which the game needs (Intel Core 2 'Penryn' from 2008 or newer, AMD 'Bulldozer' from 2011 or newer). The game cannot run here, so the installation is blocked."
    }
    return $null
}
# Windows' IsProcessorFeaturePresent, feature 37 = PF_SSE4_1_INSTRUCTIONS_AVAILABLE (winnt.h; 36 is SSSE3, 38 SSE4.2, 39 AVX, 40 AVX2).
# $null when it cannot be asked: another architecture (the x64 build is what matters; an ARM PowerShell reports no SSE), or the
# type cannot be added.
function Test-Sse41 {
    if ($env:PROCESSOR_ARCHITECTURE -notin @('AMD64', 'x86')) { return $null }
    try {
        if (-not ('GowCpu.Native' -as [type])) {
            Add-Type -Namespace GowCpu -Name Native -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern bool IsProcessorFeaturePresent(uint feature);'
        }
        return [bool][GowCpu.Native]::IsProcessorFeaturePresent(37)
    }
    catch {
        Warn "Could not check the processor for ${CpuRequirement}: $($_.Exception.Message)"
        return $null
    }
}

# --- 0. Tools -----------------------------------------------------------------------------------------------------
# GOW-Port: the installer is self-sufficient. Git, and CMake and Ninja when the system has no usable ones, are downloaded in portable
# form into toolchain\ (without installing anything on Windows or asking for administrator). Only Visual Studio Build Tools (C++) is installed, with -InstallTools,
# because Microsoft does not allow redistributing it: its official installer is downloaded and its signature is checked.
$Toolchain = Join-Path $DataRoot 'toolchain'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

# GOW-Port (issue #39): every download goes through Fetch. Invoke-WebRequest alone failed on a player's PC for every tool at
# once with "The system cannot find the file specified" (a Windows error, not a network one) while the Internet worked, so
# one broken component of Windows PowerShell's web stack ended the installation. Fetch tries four independent ways in turn:
# Invoke-WebRequest, .NET's HttpClient, the curl.exe that ships with Windows 10 1803 and later, and BITS; each result is
# checked against its SHA-256 when one is pinned. A file the player downloaded with a browser and saved at the path the
# error names is used as it is (after the same check), so a PC where nothing can download still installs.
function FileSha256([string]$file) { (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash }
function FetchWith([string]$method, [string]$url, [string]$target) {
    switch ($method) {
        'Invoke-WebRequest' { Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $target }
        'HttpClient' {
            Add-Type -AssemblyName System.Net.Http
            $client = New-Object System.Net.Http.HttpClient
            $client.Timeout = [TimeSpan]::FromMinutes(30)
            try {
                $response = $client.GetAsync($url, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
                [void]$response.EnsureSuccessStatusCode()
                $source = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
                $file = [IO.File]::Create($target)
                try { $source.CopyTo($file) } finally { $file.Dispose(); $source.Dispose() }
            }
            finally { $client.Dispose() }
        }
        'curl' {
            $curl = Join-Path $env:SystemRoot 'System32\curl.exe'
            if (-not (Test-Path -LiteralPath $curl)) { throw 'curl.exe is not part of this Windows' }
            # Its messages go to a file: native stderr would stop the script under $ErrorActionPreference = 'Stop'.
            $log = "$target.curl.txt"
            & $curl --fail --location --silent --show-error --retry 2 --connect-timeout 30 --output $target --stderr $log $url
            $code = $LASTEXITCODE
            # An empty file reads as nothing in Windows PowerShell, even cast to [string]: interpolation always gives a string.
            $message = if (Test-Path -LiteralPath $log) { "$(Get-Content -LiteralPath $log -Raw)".Trim() } else { '' }
            Remove-Item -LiteralPath $log -Force -ErrorAction SilentlyContinue
            if ($code -ne 0) { throw "curl.exe ended with code $code $message" }
        }
        'BITS' {
            Import-Module BitsTransfer -ErrorAction Stop
            Start-BitsTransfer -Source $url -Destination $target -ErrorAction Stop
        }
    }
}
function Fetch([string]$name, [string]$url, [string]$target, [string]$sha256 = '') {
    New-Item -ItemType Directory -Force -Path (Split-Path $target) | Out-Null
    if (Test-Path -LiteralPath $target) {
        if (-not $sha256 -or (FileSha256 $target) -eq $sha256) {
            Write-Host "Using $target, already on this PC."
            return
        }
        Remove-Item -LiteralPath $target -Force
    }
    $failures = @()
    foreach ($method in @('Invoke-WebRequest', 'HttpClient', 'curl', 'BITS')) {
        try {
            FetchWith $method $url $target
            if (-not (Test-Path -LiteralPath $target)) { throw 'no file was written' }
            if ($sha256) {
                $actual = FileSha256 $target
                if ($actual -ne $sha256) { throw "not the expected file (SHA-256 $actual, expected $sha256)" }
            }
            if ($failures.Count -gt 0) { Write-Host "Downloaded $name with $method." }
            return
        }
        catch {
            $failures += "${method}: $($_.Exception.Message)"
            Write-Host "  $name with ${method} failed: $($_.Exception.Message)"
            if (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue }
        }
    }
    throw "Could not download $name ($($failures -join '; ')). Check the Internet connection, or download $url with a browser, save it as $target and run the installation again"
}
function Download([string]$url, [string]$target, [string]$sha256 = '') {
    try { Fetch (Split-Path $target -Leaf) $url $target $sha256 }
    catch { Fail "$($_.Exception.Message)." }
}
function FindVs {
    $vw = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vw)) { return $null }
    return (& $vw -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
}
function CheckSignature([string]$file, [string]$signer) {
    $f = Get-AuthenticodeSignature -LiteralPath $file
    if ($f.Status -ne 'Valid' -or $f.SignerCertificate.Subject -notmatch [regex]::Escape($signer)) {
        Fail "$(Split-Path $file -Leaf) does not have a valid signature from $signer (status: $($f.Status))."
    }
}
function GetPortableGit {
    $dir = Join-Path $Toolchain 'mingit'
    if (-not (Test-Path (Join-Path $dir 'cmd\git.exe'))) {
        $v = '2.47.1'
        $zip = Join-Path $Toolchain "MinGit-$v-64-bit.zip"
        Step "Downloading portable Git $v (about 50 MB)"
        Download "https://github.com/git-for-windows/git/releases/download/v$v.windows.1/MinGit-$v-64-bit.zip" $zip `
            '50B04B55425B5C465D076CDB184F63A0CD0F86F6EC8BB4D5860114A713D2C29A'
        Expand-Archive -LiteralPath $zip -DestinationPath $dir -Force
        Remove-Item -LiteralPath $zip -Force
    }
    return (Join-Path $dir 'cmd')
}

# GOW-Port: every Visual Studio on the PC, with or without the C++ compiler. Issue #14: someone who had installed Visual Studio
# themselves still reached the Build Tools installer below, which ended with code 1 after 3 seconds, and the log said nothing about
# what was already installed.
function VsInstallations {
    $vw = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $vw)) { return @() }
    try { $all = @((@(& $vw -all -prerelease -products * -format json) -join "`n") | ConvertFrom-Json) } catch { return @() }
    foreach ($i in $all) {
        if (-not $i.installationPath) { continue }
        $cpp = Test-Path -LiteralPath (Join-Path $i.installationPath 'VC\Tools\MSVC')
        [pscustomobject]@{
            Cpp  = $cpp
            Text = "$($i.displayName) $($i.installationVersion) in $($i.installationPath): C++ compiler $(if ($cpp) { 'present' } else { 'missing' }), complete $($i.isComplete), restart pending $($i.isRebootRequired)"
        }
    }
}
function InstallVsBuildTools {
    Step 'Installing Visual Studio 2022 Build Tools (C++): asks for administrator permission; 15-30 min'
    $existing = @(VsInstallations)
    foreach ($e in $existing) { Write-Host "  Visual Studio found: $($e.Text)" }
    $bootstrap = Join-Path $Toolchain 'vs_BuildTools.exe'
    Download 'https://aka.ms/vs/17/release/vs_BuildTools.exe' $bootstrap
    CheckSignature $bootstrap 'Microsoft Corporation'
    # GOW-Port: two Visual Studio installers cannot run at once (one left open by the user makes this one end with code 1).
    $running = @(Get-Process -Name 'vs_installer', 'vs_installershell', 'vs_setup_bootstrapper', 'setup' -ErrorAction SilentlyContinue | Where-Object { $_.Path -match 'Visual Studio' })
    if ($running) { Warn "A Visual Studio installer is already open ($(($running | ForEach-Object { $_.ProcessName }) -join ', ')); close it, or this installation may end with code 1." }
    $started = Get-Date
    try {
        $p = Start-Process -FilePath $bootstrap -Verb RunAs -Wait -PassThru -ArgumentList @(
            '--wait', '--passive', '--norestart', '--add', 'Microsoft.VisualStudio.Workload.VCTools', '--includeRecommended')
    }
    catch { Fail 'Could not start the Visual Studio installer: the administrator permission must be accepted.' }
    # 3010 = installed, but Windows asks for a restart; the C++ tools can already be used.
    if ($p.ExitCode -ne 0 -and $p.ExitCode -ne 3010) {
        # GOW-Port: the installer's own logs (issue #14: "ended with code 1" says nothing); their error lines go into install.log.
        $logs = @(Get-ChildItem -Path $env:TEMP -Filter 'dd_*.log' -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -ge $started.AddMinutes(-1) } | Sort-Object LastWriteTime -Descending | Select-Object -First 4)
        foreach ($log in $logs) {
            Write-Host "  $($log.Name):"
            Select-String -LiteralPath $log.FullName -Pattern 'error|failed|exit code' -ErrorAction SilentlyContinue | Select-Object -Last 6 | ForEach-Object { Write-Host "    $($_.Line.Trim())" }
        }
        $noCpp = @($existing | Where-Object { -not $_.Cpp })
        if ($noCpp) {
            Fail "The Visual Studio installer ended with code $($p.ExitCode). A Visual Studio is already installed without the C++ compiler ($($noCpp[0].Text)): open its Visual Studio Installer, choose Modify, tick ""Desktop development with C++"", install, and run this installation again without asking for the tools. Details: $env:TEMP\dd_*.log"
        }
        Fail "The Visual Studio installer ended with code $($p.ExitCode). Close any open Visual Studio Installer window, restart Windows if an installation is pending, and try again; if you already installed Visual Studio yourself, run this installation again without asking for the tools. Details: $env:TEMP\dd_*.log"
    }
}
# GOW-Port: before anything long (the Visual Studio installation included): a processor without SSE4.1 cannot run the game.
if ($SkipCpuCheck) { Write-Host "CPU: the $CpuRequirement check was skipped (-SkipCpuCheck)." }
else {
    $cpuProblem = Get-CpuProblem (Test-Sse41)
    if ($cpuProblem) { Fail "$cpuProblem To install anyway (a virtual machine may hide a feature the CPU has), pass -SkipCpuCheck." }
}
# GOW-Port: the Rust tools (RustPortPS2/). Order for each: its parameter, then bin\<tool>.exe (the launcher's kit), then cargo,
# which needs Visual Studio's linker: so with the kit's tools the clang build needs no Visual Studio at all.
function ResolveTool([string]$given, [string]$parameter, [string]$name) {
    if ($given) {
        if (-not (Test-Path -LiteralPath $given)) { Fail "$parameter not found: $given" }
        return (Resolve-Path -LiteralPath $given).Path
    }
    $kit = Join-Path $Root "bin\$name.exe"
    if (Test-Path -LiteralPath $kit) { return $kit }
    return $null
}
$recompExe = ResolveTool $RecompExe '-RecompExe' 'ps2_recomp'
$toolsExe = ResolveTool $ToolsExe '-ToolsExe' 'ps2-tools'
$needCargo = (-not $recompExe) -or (-not $toolsExe)
$useVs = ($Compiler -eq 'msvc') -or $needCargo
if ($useVs -and $InstallTools -and -not (FindVs)) { InstallVsBuildTools }

Step 'Checking tools'
Write-Host "Compiler: $Compiler$(if ($Compiler -eq 'clang' -and $needCargo) { ' (Visual Studio loaded only to build the Rust tools with cargo)' })"
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if ($useVs -and -not (Test-Path $vswhere)) {
    if ($Compiler -eq 'msvc') { Fail 'Visual Studio (Build Tools) is not installed. Run: install.cmd -InstallTools, or use the default clang compiler.' }
    Fail 'The Rust tools (ps2_recomp, ps2-tools) are not next to this script and building them with cargo needs Visual Studio. Use the launcher''s kit (bin\), or pass -RecompExe and -ToolsExe.'
}
$vs = $null
# MSVC environment (cl, link, cmake and ninja from Visual Studio) in this PowerShell session. A temporary .cmd avoids the
# quoting problems of "cmd /c" with paths containing spaces.
# vcvars64.bat looks vswhere.exe up by name, and outside a Developer Prompt its folder is not in PATH: it printed "'vswhere.exe'
# is not recognized" (in the console's code page, which showed up garbled in the launcher's log). The folder goes into PATH
# first, and stderr is discarded too: a real failure is caught by the checks for cl, cmake and ninja right below.
# GOW-Port: a function, because installing Visual Studio 2022 next to an old one (see the cmake check) reloads it.
function LoadVsEnvironment {
    # GOW-Port: every installation with the C++ tools, the newest first (-sort), and for each one two loaders: vcvars64.bat and then
    # VsDevCmd.bat, the entry point Microsoft documents (vcvars64.bat ends up there). Issue #14: a Visual Studio 2026 (18) with the C++
    # workload where cl was not in PATH after vcvars64.bat; the loader's own output was discarded, so the cause was invisible.
    # It now goes to a file and, if no loader works, into the log.
    $instances = @(& $vswhere -products * -sort -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Where-Object { $_ })
    if (-not $instances) { Fail 'Visual Studio does not have the C++ compiler (the "Desktop development with C++" workload). Run: install.cmd -InstallTools' }
    $vsInstaller = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer'
    # The .cmd holds paths of this PC, so it is written in the console's (OEM) code page, not ASCII.
    $oem = [Text.Encoding]::GetEncoding([Globalization.CultureInfo]::CurrentCulture.TextInfo.OEMCodePage)
    $cmdFile = Join-Path $env:TEMP 'PC_vcvars.cmd'
    $outFile = Join-Path $env:TEMP 'PC_vcvars.out'
    $notes = @()
    foreach ($path in $instances) {
        $loaders = @(
            @{ Name = 'vcvars64.bat'; File = (Join-Path $path 'VC\Auxiliary\Build\vcvars64.bat'); Args = '' },
            @{ Name = 'VsDevCmd.bat'; File = (Join-Path $path 'Common7\Tools\VsDevCmd.bat'); Args = '-arch=x64 -host_arch=x64 -no_logo' })
        foreach ($l in $loaders) {
            if (-not (Test-Path -LiteralPath $l.File)) { $notes += "$path : $($l.Name) is missing"; continue }
            [IO.File]::WriteAllText($cmdFile, "@set `"PATH=$vsInstaller;%PATH%`"`r`n@call `"$($l.File)`" $($l.Args) > `"$outFile`" 2>&1`r`n@set`r`n", $oem)
            # GOW-Port: Windows' own interpreter (ComSpec), not "cmd" looked up in PATH: with devkitPro or MSYS2 first in PATH, "cmd" was
            # their usr\bin\cmd, a file that PowerShell cannot run in a pipeline (issue #23).
            & $env:ComSpec /c $cmdFile | ForEach-Object {
                if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "env:$($matches[1])" -Value $matches[2] }
            }
            # GOW-Port: only the compiler is required here. The newest Visual Studio may lack the "C++ CMake tools" component (a Visual
            # Studio 18 Community without it, issue #9): cmake and ninja are looked up in every installation, or installed, right below.
            # GOW-Port: and the Windows SDK's resource compiler. A Visual Studio 18 with the compiler but no Windows SDK passed this
            # point and then stopped inside CMake's compiler test with "cmake ended with code 1" (rc and mt not found, issue #9).
            $hasCl = [bool](Get-Command cl -ErrorAction SilentlyContinue)
            $hasRc = [bool](Get-Command rc -ErrorAction SilentlyContinue)
            if ($hasCl -and $hasRc) { $script:vs = $path; return }
            if ($hasCl) { $notes += "$path : $($l.Name) put cl in PATH but not rc.exe (the Windows SDK is not installed)"; continue }
            $tail = if (Test-Path -LiteralPath $outFile) { @(Get-Content -LiteralPath $outFile -Tail 8 -ErrorAction SilentlyContinue) } else { @() }
            $notes += "$path : $($l.Name) did not put cl in PATH; its output: $($tail -join ' | ')"
        }
    }
    Write-Host 'Visual Studio installations with the C++ tools, and what each loader did:'
    foreach ($n in $notes) { Write-Host "  $n" }
    $msvc = Join-Path $instances[0] 'VC\Tools\MSVC'
    $toolsets = if (Test-Path -LiteralPath $msvc) { @(Get-ChildItem -LiteralPath $msvc -Directory | ForEach-Object { "$($_.Name) (cl.exe: $(Test-Path -LiteralPath (Join-Path $_.FullName 'bin\Hostx64\x64\cl.exe')))" }) } else { @() }
    Write-Host "  MSVC toolsets in $($instances[0]): $(if ($toolsets) { $toolsets -join ', ' } else { 'none' })"
    # GOW-Port: vcvars64.bat is written long before the compiler (about 14 minutes in a clean Visual Studio 2026 install), so running
    # this while the Visual Studio installer is still working finds the loader without the compiler. Issue #14 showed exactly that.
    $installing = @(Get-Process -Name 'vs_installer', 'vs_installershell', 'vs_setup_bootstrapper', 'vs_BuildTools', 'vs_community', 'setup' -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -like 'vs_*' -or $_.Path -match 'Visual Studio' })
    $noSdk = (Get-Command cl -ErrorAction SilentlyContinue) -and -not (Get-Command rc -ErrorAction SilentlyContinue)
    if ($noSdk -and -not $installing) {
        Fail "The C++ compiler is installed ($($instances[0])) but the Windows SDK is not (rc.exe was not found), and the build needs it. Open the Visual Studio Installer, choose Modify, and tick a Windows SDK (for example ""Windows 11 SDK"") under ""Desktop development with C++"" or ""Individual components""; then run this installation again. Finished steps are kept."
    }
    if ($installing) {
        Fail "A Visual Studio installer is open or still working ($(($installing | ForEach-Object { $_.ProcessName } | Sort-Object -Unique) -join ', ')), and the C++ compiler is not there yet. Wait for the installation to finish (the Visual Studio Installer window shows its progress), restart Windows if it asks, and run this installation again."
    }
    Fail "cl not found after loading vcvars64.bat or VsDevCmd.bat ($($instances[0])). Open the Visual Studio Installer, choose Modify and check that ""Desktop development with C++"" is installed with an ""MSVC build tools"" component and a Windows SDK; the details are above in install.log."
}
if ($useVs) { LoadVsEnvironment }
# GOW-Port: the build needs a CMake 3.21 or newer with cmcldeps.exe next to it, and the first one in PATH may not be that:
# - an older one (an old Visual Studio's, Android SDK, another recompilation kit) does not understand "-S <src> -B <build>": it takes
#   the last path as the source and stops with "The source directory <build> does not exist";
# - one from another toolchain can be 3.21+ and lack cmcldeps.exe: the Ninja generator then writes the resource compiler rule (rc.exe)
#   with an empty helper and the build fails with RC1107 near its end.
# The CMake and Ninja bundled with Visual Studio are the pair Microsoft tests with the MSVC compiler used here, so they are preferred
# when usable; otherwise the first usable CMake in PATH is used, and only if there is none is something installed (-InstallTools).
function CmakeVersion([string]$cmake = 'cmake') {
    $m = [regex]::Match((& $cmake --version | Select-Object -First 1), '(\d+)\.(\d+)')
    if (-not $m.Success) { return [version]'0.0' }
    return [version]"$($m.Groups[1].Value).$($m.Groups[2].Value)"
}
function CmakeUsable([string]$cmake = 'cmake') {
    $source = (Get-Command $cmake).Source
    return ((CmakeVersion $source) -ge [version]'3.21') -and (Test-Path -LiteralPath (Join-Path (Split-Path -Parent $source) 'cmcldeps.exe'))
}
# Puts the chosen CMake first in PATH (with Visual Studio's Ninja when it is Visual Studio's CMake) and returns its path, or $null.
function UseUsableCmake {
    # GOW-Port: every Visual Studio installation, the newest first, not only the one whose vcvars64.bat was loaded: a newer one
    # without the CMake component must not hide the CMake of Visual Studio 2022 Build Tools installed next to it.
    $vsPaths = if (Test-Path -LiteralPath $vswhere) { @(& $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Where-Object { $_ }) } else { @() }
    $bundledCmakes = @{}
    $candidates = @()
    foreach ($p in $vsPaths) {
        $vsCmake = Join-Path $p 'Common7\IDE\CommonExtensions\Microsoft\CMake'
        $bundled = Join-Path $vsCmake 'CMake\bin\cmake.exe'
        if (Test-Path -LiteralPath $bundled) { $candidates += $bundled; $bundledCmakes[$bundled] = $vsCmake }
    }
    $candidates += @(Get-Command cmake -All -ErrorAction SilentlyContinue | ForEach-Object { $_.Source })
    foreach ($candidate in $candidates) {
        if (CmakeUsable $candidate) {
            $env:Path = (Split-Path -Parent $candidate) + ';' + $env:Path
            if ($bundledCmakes.ContainsKey($candidate)) {
                $ninja = Join-Path $bundledCmakes[$candidate] 'Ninja'
                if (Test-Path -LiteralPath (Join-Path $ninja 'ninja.exe')) { $env:Path = $ninja + ';' + $env:Path }
            }
            if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) {
                foreach ($p in $vsPaths) {
                    $ninja = Join-Path $p 'Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja'
                    if (Test-Path -LiteralPath (Join-Path $ninja 'ninja.exe')) { $env:Path = $ninja + ';' + $env:Path; break }
                }
            }
            return $candidate
        }
    }
    return $null
}
$cmakeFirst = Get-Command cmake -ErrorAction SilentlyContinue
$cmakeBefore = if ($cmakeFirst) { $cmakeFirst.Source } else { 'none' }
$cmakeBeforeVersion = if ($cmakeFirst) { CmakeVersion } else { 'not installed' }
$cmakeWasUsable = [bool]$cmakeFirst -and (CmakeUsable)
$cmakeChosen = UseUsableCmake
# GOW-Port: portable CMake and Ninja (issue #11), like the portable Git: downloaded into toolchain\ from their official releases,
# SHA-256 checked, without installing anything on Windows or asking for administrator. They are used only when the system has
# no usable CMake (3.21+ with cmcldeps.exe) or no Ninja, so nobody has to add Visual Studio components or install a second
# Visual Studio just for them. The C++ compiler still comes from Visual Studio Build Tools, which Microsoft does not allow
# redistributing. A failed download is not fatal here: the other options below still apply.
function GetPortableTool([string]$name, [string]$url, [string]$sha256, [string]$zip, [string]$dir, [string]$exe) {
    if (Test-Path -LiteralPath $exe) { return $exe }
    try {
        New-Item -ItemType Directory -Force -Path $Toolchain | Out-Null
        Step "Downloading portable $name"
        Fetch $name $url $zip $sha256.ToUpperInvariant()
        if (Test-Path -LiteralPath $dir) { Remove-Item -LiteralPath $dir -Recurse -Force }
        Expand-Archive -LiteralPath $zip -DestinationPath $dir -Force
        if (-not (Test-Path -LiteralPath $exe)) { throw "$exe is missing after extracting $name" }
        return $exe
    }
    catch {
        Warn "Could not set up portable $name ($($_.Exception.Message))."
        return $null
    }
    finally { if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue } }
}
function UsePortableCmake {
    $v = '3.31.6'
    $exe = GetPortableTool 'CMake' "https://github.com/Kitware/CMake/releases/download/v$v/cmake-$v-windows-x86_64.zip" `
        'd163cd3ab4959b0a53fa8988f2ddbd2e6c501658201e6a154386bad9dbe4f836' (Join-Path $Toolchain "cmake-$v.zip") `
        (Join-Path $Toolchain "cmake-$v") (Join-Path $Toolchain "cmake-$v\cmake-$v-windows-x86_64\bin\cmake.exe")
    if ($exe -and (CmakeUsable $exe)) {
        $env:Path = (Split-Path -Parent $exe) + ';' + $env:Path
        return $exe
    }
    return $null
}
function UsePortableNinja {
    $v = '1.12.1'
    $exe = GetPortableTool 'Ninja' "https://github.com/ninja-build/ninja/releases/download/v$v/ninja-win.zip" `
        'f550fec705b6d6ff58f2db3c374c2277a37691678d6aba463adcbb129108467a' (Join-Path $Toolchain "ninja-$v.zip") `
        (Join-Path $Toolchain "ninja-$v") (Join-Path $Toolchain "ninja-$v\ninja.exe")
    if ($exe) { $env:Path = (Split-Path -Parent $exe) + ';' + $env:Path }
}
if (-not $cmakeChosen) { $cmakeChosen = UsePortableCmake }
# An old Visual Studio (2017, 2019 before 16.10) bundles an old CMake. With -InstallTools (the launcher's wizard, which asked the
# user), Visual Studio 2022 is installed next to it: it becomes the newest, and the old one stays. Only if the portable CMake
# could not be downloaded.
if (-not $cmakeChosen -and $InstallTools -and $useVs) {
    Warn 'No usable CMake (3.21 or newer with cmcldeps.exe) was found: installing Visual Studio 2022 Build Tools next to the current Visual Studio.'
    InstallVsBuildTools
    LoadVsEnvironment
    $cmakeChosen = UseUsableCmake
}
if (-not $cmakeChosen) {
    Fail "A CMake 3.21 or newer with cmcldeps.exe next to it is required, but $cmakeBefore ($cmakeBeforeVersion) does not meet that and no other was found (Visual Studio: $vs). Visual Studio needs the ""C++ CMake tools for Windows"" component (Visual Studio Installer > Modify), an old Visual Studio (2017, 2019 before 16.10) bundles an old CMake: install Visual Studio 2022 Build Tools (install.cmd -InstallTools), or put an official CMake 3.21+ first in PATH."
}
if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) { UsePortableNinja }
if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) {
    Fail 'ninja was not found and the portable one could not be downloaded: check the Internet connection, add the "C++ CMake tools for Windows" component to Visual Studio (Visual Studio Installer > Modify), or put ninja in PATH.'
}
if ($cmakeChosen -ne $cmakeBefore) {
    $note = "CMake: using $cmakeChosen instead of $cmakeBefore ($cmakeBeforeVersion)."
    if ($cmakeWasUsable) { Write-Host $note } else { Warn "$note It is older than 3.21 or lacks cmcldeps.exe." }
}
# GOW-Port: the first configuration downloads raylib, imgui and rlImGui with Git; with the dependencies already downloaded
# (or with Git installed) nothing is needed.
if (-not (Get-Command git -ErrorAction SilentlyContinue) -and -not (Test-Path (Join-Path $Build '_deps\raylib-src'))) {
    $env:Path = (GetPortableGit) + ';' + $env:Path
}

# GOW-Port: llvm-mingw (clang + lld + llvm-rc, UCRT), the game's compiler by default. One pinned release, its SHA-256 checked,
# unpacked into toolchain\ without the ARM and 32-bit targets nobody uses here (about half of its ~1.5 GB). The same version for
# every player, so a build does not depend on which Visual Studio, SDK or system compiler happens to be installed. The pin is the
# release the gow-native-pc project measured (Clang 23); changing it is a decision measured with scripts\ab-perf.ps1.
$ClangVersion = '20261020'
$ClangUrl = "https://github.com/mstorsjo/llvm-mingw/releases/download/$ClangVersion/llvm-mingw-$ClangVersion-ucrt-x86_64.zip"
$ClangSha256 = 'FD5462B7F5D93B2FF981FCD3F9C7E427E47F3203C64EA91C27BCC444EEBD74CD'
function ExpandZipFiltered([string]$zip, [string]$dir, [string[]]$skip) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($zip)
    try {
        foreach ($entry in $archive.Entries) {
            $name = $entry.FullName -replace '\\', '/'
            if ($name.EndsWith('/')) { continue }
            $skipped = $false
            foreach ($pattern in $skip) { if ($name -like $pattern) { $skipped = $true; break } }
            if ($skipped) { continue }
            $target = Join-Path $dir $name
            $parent = Split-Path -Parent $target
            if (-not (Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
        }
    }
    finally { $archive.Dispose() }
}
function UsePortableClang {
    $dir = Join-Path $Toolchain "llvm-mingw-$ClangVersion"
    $bin = Join-Path $dir "llvm-mingw-$ClangVersion-ucrt-x86_64\bin"
    if (Test-Path -LiteralPath (Join-Path $bin 'clang++.exe')) { return $bin }
    $zip = Join-Path $Toolchain "llvm-mingw-$ClangVersion.zip"
    New-Item -ItemType Directory -Force -Path $Toolchain | Out-Null
    $lastError = ''
    # Two attempts: a dropped connection on a 170 MB download should not end the installation.
    for ($attempt = 1; $attempt -le 2; $attempt++) {
        try {
            Step "Downloading the compiler (llvm-mingw $ClangVersion, about 170 MB$(if ($attempt -gt 1) { ', second attempt' }))"
            Fetch "the compiler (llvm-mingw $ClangVersion)" $ClangUrl $zip $ClangSha256
            Write-Host 'Unpacking the compiler...'
            if (Test-Path -LiteralPath $dir) { Remove-Item -LiteralPath $dir -Recurse -Force }
            ExpandZipFiltered $zip $dir @('*/aarch64-w64-mingw32/*', '*/armv7-w64-mingw32/*', '*/i686-w64-mingw32/*', '*/python/*')
            if (-not (Test-Path -LiteralPath (Join-Path $bin 'clang++.exe'))) {
                throw "clang++.exe is missing after unpacking (an antivirus may have removed it; allow the folder $Toolchain and try again)"
            }
            return $bin
        }
        catch { $lastError = $_.Exception.Message }
        finally { if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue } }
    }
    Fail "Could not set up the compiler: $lastError. Finished steps are kept when the installation runs again."
}
$ClangBin = $null
if ($Compiler -eq 'clang') {
    $ClangBin = UsePortableClang
    # GOW-Port: the build sees only its own tools and Windows: MSYS2, devkitPro, Cygwin, another project's CMake or an old
    # compiler first in PATH made builds fail in ways that had nothing to do with the game (issues #1, #23). cargo stays when the
    # Rust tools still have to be built (it runs before the game build, with Visual Studio's environment).
    $keep = @($ClangBin)
    # A Unix-like environment's folder (MSYS2, Cygwin, devkitPro) brings its own gcc, cmd, make and DLLs with the tool: never
    # one of those. Without a clean Git, the portable MinGit (which install.ps1 already knows how to fetch) is used.
    $foreign = '[\\/](msys64|msys32|msys2|cygwin64|cygwin|devkitpro|mingw64|ucrt64|clang64)([\\/]|$)'
    foreach ($tool in @('cmake', 'ninja', 'git')) {
        $found = @(Get-Command $tool -All -ErrorAction SilentlyContinue | Where-Object { $_.Source -notmatch $foreign }) | Select-Object -First 1
        if ($found) { $keep += (Split-Path -Parent $found.Source) }
        elseif ($tool -eq 'git' -and -not (Test-Path (Join-Path $Build '_deps\raylib-src'))) { $keep += (GetPortableGit) }
    }
    if ($needCargo) {
        $cargo = Get-Command cargo -ErrorAction SilentlyContinue
        if ($cargo) { $keep += (Split-Path -Parent $cargo.Source) }
    }
    $system = Join-Path $env:SystemRoot 'System32'
    $keep += @($system, $env:SystemRoot, (Join-Path $system 'Wbem'), (Join-Path $system 'WindowsPowerShell\v1.0'))
    if (-not $needCargo) { $env:Path = (($keep | Select-Object -Unique) -join ';') }
    else { $env:Path = (($keep | Select-Object -Unique) -join ';') + ';' + $env:Path }
}

# GOW-Port: the build environment in the log, to diagnose reports without asking for it.
if ($useVs) {
    Write-Host "Visual Studio: $vs ($((& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property catalog_productDisplayVersion)))"
}
$toolNames = if ($Compiler -eq 'clang') { @('clang++', 'cmake', 'ninja') } else { @('cl', 'cmake', 'ninja') }
foreach ($c in $toolNames) {
    $cmd = Get-Command $c
    # cl prints its banner on stderr, which would abort under $ErrorActionPreference = 'Stop': its file version is used.
    $ver = if ($c -eq 'cl') { $cmd.Version } else { (& $c --version | Select-Object -First 1) }
    Write-Host "  ${c}: $($cmd.Source) ($ver)"
}

# Both Rust tools come out of one cargo build when either is missing.
if ($needCargo) {
    Step 'Building the Rust tools (ps2_recomp, ps2-tools)'
    $rustRoot = Join-Path $Root 'RustPortPS2'
    if (-not (Test-Path -LiteralPath (Join-Path $rustRoot 'third_party\rabbitizer\Cargo.toml'))) {
        Fail 'RustPortPS2\third_party\rabbitizer is missing: the repository checkout is incomplete.'
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Fail 'cargo was not found: install Rust (https://rustup.rs), or pass -RecompExe and -ToolsExe with built executables.'
    }
    $rustTarget = Join-Path $DataRoot 'build-rust'
    Run 'cargo' @('build', '--release', '--locked', '-p', 'ps2-recomp-cli', '-p', 'ps2-tools',
        '--manifest-path', (Join-Path $rustRoot 'Cargo.toml'), '--target-dir', $rustTarget)
    if (-not $recompExe) { $recompExe = Join-Path $rustTarget 'release\ps2_recomp.exe' }
    if (-not $toolsExe) { $toolsExe = Join-Path $rustTarget 'release\ps2-tools.exe' }
}
Write-Host "Using the recompiler: $recompExe"
Write-Host "Using ps2-tools: $toolsExe"

# --- 1. Game data -------------------------------------------------------------------------------------------------
Step 'Game data (game\)'
New-Item -ItemType Directory -Force -Path $Game | Out-Null
# GOW-Port: custom textures (launcher switch): PCSX2's layout, the pack goes inside textures\replacements.
New-Item -ItemType Directory -Force -Path (Join-Path $DataRoot 'textures\replacements') | Out-Null
if (-not (Test-Path (Join-Path $Game 'SCUS_973.99')) -or -not (Test-Path (Join-Path $Game '.disc_layout'))) {
    if (-not $Iso) {
        $c = Get-ChildItem -Path $Game, $Root -Filter *.iso -File -ErrorAction SilentlyContinue | Select-Object -First 1
        # GOW-Port (0.3.1): a CHD disc image (chdman) is accepted too.
        if (-not $c) { $c = Get-ChildItem -Path $Game, $Root -Filter *.chd -File -ErrorAction SilentlyContinue | Select-Object -First 1 }
        if ($c) { $Iso = $c.FullName }
    }
    if (-not $Iso -or -not (Test-Path -LiteralPath $Iso)) {
        Fail 'God of War ISO (USA, SCUS-97399) not found. Put it in game\ or use: install.cmd -Iso "C:\path\God of War.iso" (a .chd works too)'
    }
    # GOW-Port (0.3.1): a CHD is first converted to a temporary ISO in the destination (every hunk and its SHA-1 checked;
    # the CHD is only read), which is deleted once the disc is extracted. A leftover from a failed run is replaced.
    $tempIso = $null
    if ($Iso -match '\.chd$') {
        $tempIso = Join-Path $DataRoot 'disc-from-chd.iso'
        Remove-Item -LiteralPath $tempIso -Force -ErrorAction SilentlyContinue
        Write-Host "Converting $Iso to a temporary ISO (about 8 GB, deleted after the extraction)..."
        Run $toolsExe @('chd-extract', $Iso, $tempIso)
        $Iso = $tempIso
    }
    Write-Host "Extracting $Iso (about 8 GB; the game does not need the ISO afterwards)..."
    Run $toolsExe @('iso-extract', $Iso, $Game, '--layout')
    if ($tempIso) { Remove-Item -LiteralPath $tempIso -Force -ErrorAction SilentlyContinue }
}
$cnf = Join-Path $Game 'SYSTEM.CNF'
if (-not (Test-Path $cnf) -or -not (Select-String -Path $cnf -Pattern 'SCUS_973.99' -Quiet)) {
    Fail 'game\ is not God of War USA (SCUS-97399): SYSTEM.CNF does not boot SCUS_973.99.'
}
$iopMod = Join-Path $Game 'IOP_MOD'
# GOW-Port: a correct file name does not identify the executable's revision.
$verifyArgs = @('verify-elf', (Join-Path $Game 'SCUS_973.99'), '--map', (Join-Path $Profile 'funcmap.csv'))
if ($AllowModifiedElf) { $verifyArgs += '--allow-modified' }
Run $toolsExe $verifyArgs
# GOW-Port (R5): the memory card is created already formatted, laid out like a card the PS2 BIOS formats; the game's own
# formatting of a blank card was never verified. An existing card (your saves) is never touched.
$card = Join-Path $Game 'Mcd001.ps2'
if (-not (Test-Path -LiteralPath $card)) {
    & $toolsExe mc-format $card
    if ($LASTEXITCODE -ne 0) { Warn 'Could not create the memory card; the game will start with a blank one.' }
}
New-Item -ItemType Directory -Force -Path $iopMod | Out-Null
Get-ChildItem -Path $Game -Filter *.IRX -File | ForEach-Object { Copy-Item $_.FullName (Join-Path $iopMod $_.Name) -Force }
Write-Host 'Done: IOP modules in game\IOP_MOD.'

# GOW-Port: the VU1 microcode is in the ELF itself. The manifest (games/scus-97399/vu1.manifest) says which piece of the
# ELF goes at each micro-memory address, and here the micro-memories are assembled without playing. That way the first build already
# carries native VU1 and the capture by playing (stage 4) is skipped. If it cannot be applied (another ELF), that capture and the
# launcher's progressive one remain.
$manifest = Join-Path $Profile 'vu1.manifest'
if ((Test-Path -LiteralPath $manifest) -and $Vu1ForBuild -eq $Vu1 -and
    -not (Get-ChildItem -Path $Vu1 -Filter *.bin -ErrorAction SilentlyContinue | Select-Object -First 1)) {
    Step 'VU1 microcode from the ELF (without playing)'
    New-Item -ItemType Directory -Force -Path $Vu1 | Out-Null
    & $toolsExe vu1-manifest apply (Join-Path $Game 'SCUS_973.99') $manifest $Vu1
    if ($LASTEXITCODE -ne 0) { Warn 'Could not apply the VU1 manifest to this ELF; the microcode will be captured by playing.' }
}
# GOW-Port: VU1 code that the ELF declares (.DVP.ovlytab) and that no capture has seen uploaded: vu1s_*.bin images
# (ps2-tools vu1-programs), whose positions the generator takes all as possible block entries. Idempotent.
if ((Test-Path -LiteralPath $manifest) -and $Vu1ForBuild -eq $Vu1 -and
    (Get-ChildItem -Path $Vu1 -Filter vu1_*.bin -ErrorAction SilentlyContinue | Select-Object -First 1)) {
    & $toolsExe vu1-programs apply (Join-Path $Game 'SCUS_973.99') $manifest $Vu1
}

# --- 2. Recompiler and game code ----------------------------------------------------------------------------------
function Configure {
    # GOW-Port: a build folder configured from another source tree (a checkout, an older kit) makes CMake stop with
    # "does not match the source". Its cache is dropped; the downloaded dependencies (_deps) are kept.
    $cache = Join-Path $Build 'CMakeCache.txt'
    if (Test-Path -LiteralPath $cache) {
        $cachedSource = Select-String -LiteralPath $cache -Pattern '^CMAKE_HOME_DIRECTORY:INTERNAL=(.*)$' | Select-Object -First 1
        if ($cachedSource -and ((Slashes $cachedSource.Matches[0].Groups[1].Value).TrimEnd('/') -ine (Slashes $Native).TrimEnd('/'))) {
            Warn "The build folder was configured from another source ($($cachedSource.Matches[0].Groups[1].Value)): reconfiguring."
            Remove-Item -LiteralPath $cache -Force
            Remove-Item -LiteralPath (Join-Path $Build 'CMakeFiles') -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
    # GOW-Port: the cache also remembers the full path of the C++ compiler it found. A Visual Studio update replaces the MSVC
    # toolset folder (...\MSVC\14.38.x\bin\...\cl.exe), and a build folder made before it then stops with "not a full path to an
    # existing compiler". That is what an old installation (launcher 0.1.0 or 0.2.x) meets when its game is rebuilt after an
    # update, so the cache is dropped here too; again the downloaded dependencies (_deps) are kept.
    if (Test-Path -LiteralPath $cache) {
        $cachedCompiler = Select-String -LiteralPath $cache -Pattern '^CMAKE_CXX_COMPILER:[A-Z]+=(.*)$' | Select-Object -First 1
        if ($cachedCompiler) {
            # GOW-Port: not $compiler: PowerShell names ignore case, and that local hid the -Compiler parameter (clang), so a
            # build folder made with Visual Studio was kept and the game was built with MSVC.
            $cachedPath = $cachedCompiler.Matches[0].Groups[1].Value.Trim()
            # An absolute path that is not there. A bare name (cl, c++) is looked up in PATH by CMake itself.
            if ($cachedPath -match '^(?:[A-Za-z]:[\\/]|/)' -and -not (Test-Path -LiteralPath $cachedPath)) {
                Warn "The build folder remembers a C++ compiler that is gone ($cachedPath): reconfiguring."
                Remove-Item -LiteralPath $cache -Force
                Remove-Item -LiteralPath (Join-Path $Build 'CMakeFiles') -Recurse -Force -ErrorAction SilentlyContinue
            }
        }
    }
    $extra = @()
    # GOW-Port: clang (default): the pinned llvm-mingw, and llvm-rc for the icon. A build folder configured with another compiler
    # (an installation made with Visual Studio before 0.3.0) is reconfigured; the downloaded dependencies (_deps) are kept.
    if ($Compiler -eq 'clang') {
        $wantCxx = Slashes (Join-Path $ClangBin 'clang++.exe')
        if (Test-Path -LiteralPath $cache) {
            $cachedCxx = Select-String -LiteralPath $cache -Pattern '^CMAKE_CXX_COMPILER:[A-Z]+=(.*)$' | Select-Object -First 1
            if ($cachedCxx -and ((Slashes $cachedCxx.Matches[0].Groups[1].Value.Trim()) -ine $wantCxx)) {
                Write-Host "The build folder was configured with another compiler ($($cachedCxx.Matches[0].Groups[1].Value.Trim())): reconfiguring for clang."
                Remove-Item -LiteralPath $cache -Force
                Remove-Item -LiteralPath (Join-Path $Build 'CMakeFiles') -Recurse -Force -ErrorAction SilentlyContinue
                Remove-Item -LiteralPath (Join-Path $Build 'gow-configure.sig') -Force -ErrorAction SilentlyContinue
            }
        }
        $extra += "-DCMAKE_C_COMPILER=$(Slashes (Join-Path $ClangBin 'clang.exe'))"
        $extra += "-DCMAKE_CXX_COMPILER=$wantCxx"
        $extra += "-DCMAKE_RC_COMPILER=$(Slashes (Join-Path $ClangBin 'llvm-rc.exe'))"
    }
    elseif (Test-Path -LiteralPath $cache) {
        # GOW-Port: -Compiler msvc after a clang build: CMake is given no compiler then, so the folder would keep clang.
        $cachedCxx = Select-String -LiteralPath $cache -Pattern '^CMAKE_CXX_COMPILER:[A-Z]+=(.*)$' | Select-Object -First 1
        if ($cachedCxx -and $cachedCxx.Matches[0].Groups[1].Value -match 'clang') {
            Write-Host "The build folder was configured with clang ($($cachedCxx.Matches[0].Groups[1].Value.Trim())): reconfiguring for Visual Studio."
            Remove-Item -LiteralPath $cache -Force
            Remove-Item -LiteralPath (Join-Path $Build 'CMakeFiles') -Recurse -Force -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath (Join-Path $Build 'gow-configure.sig') -Force -ErrorAction SilentlyContinue
        }
    }
    # GOW-Port: PGO mode of this build ($script:PgoMode: generate, use (the profile trained here), shipped (the runtime-only
    # profile of the repository) or empty), see the end of this script.
    $pgoSignature = ''
    if ($script:PgoMode -eq 'generate') { $extra += '-DPS2X_PGO=generate' }
    elseif ($script:PgoMode -eq 'use') {
        $extra += '-DPS2X_PGO=use'
        $extra += "-DPS2X_PGO_PROFILE=$(Slashes $PgoProfile)"
        $pgoSignature = (Get-FileHash -LiteralPath $PgoProfile -Algorithm SHA256).Hash
    }
    elseif ($script:PgoMode -eq 'shipped') {
        $extra += '-DPS2X_PGO=shipped'
        $pgoSignature = 'shipped-' + (Get-FileHash -LiteralPath $ShippedProfile -Algorithm SHA256).Hash
    }
    else { $extra += '-DPS2X_PGO=off' }
    # GOW-Port: Release and RelWithDebInfo both build with /O2 and no whole-program optimization (PS2X_ENABLE_LTCG, off by default).
    $buildType = if ($FastDev) { 'RelWithDebInfo' } else { 'Release' }
    $runtimeLogs = if ($FastDev) { 'ON' } else { 'OFF' }
    $console = if ($FastDev) { 'ON' } else { 'OFF' }
    if (Get-ChildItem -Path $Vu1ForBuild -Filter *.bin -ErrorAction SilentlyContinue | Select-Object -First 1) {
        $extra += "-DPS2X_VU1_MICROCODE_DIR=$(Slashes $Vu1ForBuild)"
        $extra += "-DPS2X_VU1_GENERATOR_SOURCE=$(Slashes (Join-Path $Native 'tools\vu1\generate_vu1.cpp'))"
        $extra += "-DPS2X_TOOLS_EXE=$(Slashes $toolsExe)"
        $extra += "-DPS2X_VU1_MAX_MEMORY_MB=$($Vu1MemoryLimitGB * 1024)"
        # GOW-Port: CMake reads vu1n_heads.txt (dynamic VU1 entry points) next to the microcode folder; its source in the
        # repository is games/scus-97399/vu1-heads.list.
        # Without it those blocks are interpreted: ~7 million native block misses per 5 s in a fight.
        $headsSource = Join-Path $Profile 'vu1-heads.list'
        $headsTarget = Join-Path (Split-Path -Parent $Vu1ForBuild) 'vu1n_heads.txt'
        # GOW-Port: plus the block entries the game reached without native code while capturing (vu1n_heads_seen.txt, written
        # by the runtime next to the captures): a pc entered through a register jump is no head the generator finds alone.
        # Rewritten only when the list changes, so an unchanged list does not rebuild the VU1 code.
        $heads = New-Object System.Collections.Generic.SortedSet[int]
        foreach ($file in @($headsSource, (Join-Path $Vu1ForBuild 'vu1n_heads_seen.txt'))) {
            if (-not (Test-Path -LiteralPath $file)) { continue }
            foreach ($line in Get-Content -LiteralPath $file) {
                $text = $line.Trim()
                if ($text -match '^[0-9A-Fa-f]+$') { [void]$heads.Add([Convert]::ToInt32($text, 16)) }
            }
        }
        if ($heads.Count -gt 0) {
            $merged = (($heads | ForEach-Object { '{0:x}' -f $_ }) -join "`n") + "`n"
            $current = if (Test-Path -LiteralPath $headsTarget) { [IO.File]::ReadAllText($headsTarget) -replace "`r`n", "`n" } else { '' }
            if ($current -ne $merged) { [IO.File]::WriteAllText($headsTarget, $merged, (New-Object Text.UTF8Encoding $false)) }
        }
    }
    $cmakeArgs = @('-S', $Native, '-B', $Build, '-G', 'Ninja', "-DCMAKE_BUILD_TYPE=$buildType",
        "-DPS2X_ENABLE_RUNTIME_LOGS=$runtimeLogs", '-DPS2X_ENABLE_AGRESSIVE_LOGS=OFF', '-DPS2X_ENABLE_IOP_RPC_TRACE=OFF',
        '-DPS2X_BUILD_TEST=OFF', "-DPS2X_SHOW_WINDOWS_CONSOLE=$console",
        '-DPS2X_RUNNER_UNITY_BUILD_BATCH_SIZE=8') + $extra
    # GOW-Port: a rebuild ("Recompilar agora", -Optimize) with the same arguments skips the configure step. Ninja runs CMake
    # again by itself when a CMakeLists.txt or a CONFIGURE_DEPENDS glob changed, so only what CMake reads at configure time
    # alone goes into the signature: the arguments, the CMake executable and whether vu1n_heads.txt exists (its contents are a
    # build dependency). Idea from gow-native-pc (build.ps1 Configure -MaySkip, gow-configure.sig).
    $signatureFile = Join-Path $Build 'gow-configure.sig'
    $headsExists = Test-Path -LiteralPath (Join-Path (Split-Path -Parent $Vu1ForBuild) 'vu1n_heads.txt')
    $cmakeExe = (Get-Command cmake -ErrorAction SilentlyContinue).Source
    $signature = (@("cmake=$cmakeExe", "heads=$headsExists", "pgo=$pgoSignature") + $cmakeArgs) -join "`n"
    $previous = if (Test-Path -LiteralPath $signatureFile) { [IO.File]::ReadAllText($signatureFile) } else { $null }
    if ((Test-Path -LiteralPath (Join-Path $Build 'build.ninja')) -and (Test-Path -LiteralPath $cache) -and $previous -eq $signature) {
        Write-Host 'Configure: same arguments as the last time, skipped (ninja reconfigures by itself if the CMake files changed).'
    }
    else {
        Run 'cmake' $cmakeArgs
        [IO.File]::WriteAllText($signatureFile, $signature, (New-Object Text.UTF8Encoding $false))
    }
    # GOW-Port: a build folder that kept another compiler would build the whole game with it and without the speed profile,
    # silently (it happened with a folder made by Visual Studio). Checked here, after every configure.
    if ($Compiler -eq 'clang') {
        $usedCxx = Select-String -LiteralPath $cache -Pattern '^CMAKE_CXX_COMPILER:[A-Z]+=(.*)$' | Select-Object -First 1
        if (-not $usedCxx -or (Slashes $usedCxx.Matches[0].Groups[1].Value.Trim()) -ine $wantCxx) {
            Remove-Item -LiteralPath $signatureFile -Force -ErrorAction SilentlyContinue
            Fail "The build folder uses another compiler ($(if ($usedCxx) { $usedCxx.Matches[0].Groups[1].Value.Trim() } else { 'none' })) instead of $wantCxx. Delete $cache and run the installer again."
        }
    }
}
# GOW-Port: PGO. Two profiles: the full one trained on this PC from this game (the optional "Optimize now" / -Pgo on; it stays
# in the build folder, like the generated code, and wins when present) and the runtime-only one shipped in the repository
# (built from our own runtime, holds nothing from the game; the default, so an installation never opens the game by itself).
$PgoDir = Join-Path $Build 'pgo'
$PgoProfile = Join-Path $PgoDir 'gow.profdata'
$ShippedProfile = Join-Path $Native 'pgo\runtime.profdata'
$pgoPossible = ($Compiler -eq 'clang') -and -not $FastDev -and $Pgo -ne 'off'
$hasProfile = Test-Path -LiteralPath $PgoProfile
$hasShipped = Test-Path -LiteralPath $ShippedProfile
$trainPgo = $pgoPossible -and (($Pgo -eq 'on' -and -not $hasProfile) -or $RetrainPgo)
$script:PgoMode = if ($trainPgo) { 'generate' } elseif ($pgoPossible -and $hasProfile) { 'use' } elseif ($pgoPossible -and $hasShipped) { 'shipped' } else { '' }
if ($Compiler -eq 'clang') {
    $pgoState = switch ($script:PgoMode) {
        'generate' { 'training the full profile in the optimization run' }
        'use' { "the profile trained on this PC ($PgoProfile)" }
        'shipped' { "the runtime-only profile of the repository ($ShippedProfile); the launcher's Optimize now trains a full one" }
        default { "none ($(if ($Pgo -eq 'off') { '-Pgo off' } elseif ($FastDev) { '-FastDev' } else { 'no profile in the repository' }))" }
    }
    Write-Host "Speed profile (PGO): $pgoState"
}
Step 'Configuring the build (the first time it downloads raylib, imgui and rlImGui)'
Configure
Step 'Translating SCUS_973.99 to C++'
# ps2_recomp does not open paths with non-ASCII characters: if the repository folder has them, work happens in C:\PCGen.
$Gen = Join-Path $DataRoot 'build-gen'
if ($Gen -match '[^\x20-\x7E]') { $Gen = Join-Path $env:SystemDrive 'PCGen' }
$out = Join-Path $Gen 'out'
New-Item -ItemType Directory -Force -Path $out | Out-Null
Copy-Item (Join-Path $Game 'SCUS_973.99') (Join-Path $Gen 'SCUS_973.99') -Force
Copy-Item (Join-Path $Profile 'funcmap.csv') (Join-Path $Gen 'funcmap.csv') -Force
(Get-Content -LiteralPath (Join-Path $Profile 'gow-port.toml') -Raw) `
    -replace '@ELF@', (Slashes (Join-Path $Gen 'SCUS_973.99')) `
    -replace '@MAP@', (Slashes (Join-Path $Gen 'funcmap.csv')) `
    -replace '@OUT@', ((Slashes $out) + '/') | Set-Content -LiteralPath (Join-Path $Gen 'recomp.toml') -Encoding ASCII
$recompLog = Join-Path $Gen 'recomp.log'
& $recompExe (Join-Path $Gen 'recomp.toml') *> $recompLog
if ($LASTEXITCODE -ne 0) { Get-Content $recompLog -Tail 20; Fail 'ps2_recomp failed (see recomp.log)' }
$runner = Join-Path $Native 'ps2xRuntime\src\runner'
New-Item -ItemType Directory -Force -Path $runner | Out-Null
function Get-Sha256([string]$path) {
    # GOW-Port: Windows PowerShell 5.1 may not expose Get-FileHash in restricted shells.
    $stream = [System.IO.File]::OpenRead($path)
    $hasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([System.BitConverter]::ToString($hasher.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
    }
    finally {
        $hasher.Dispose()
        $stream.Dispose()
    }
}

function CopyIfChanged([string]$source, [string]$target) {
    # Only what changed: recompilation stays incremental.
    if ((Test-Path -LiteralPath $target) -and
        (Get-Sha256 $source) -eq (Get-Sha256 $target)) { return }
    Copy-Item -LiteralPath $source -Destination $target -Force
}
Get-ChildItem -Path $out -Filter *.cpp -File | ForEach-Object { CopyIfChanged $_.FullName (Join-Path $runner $_.Name) }
Get-ChildItem -Path $out -Filter *.h -File | ForEach-Object { CopyIfChanged $_.FullName (Join-Path $Native "ps2xRuntime\include\$($_.Name)") }
# The overrides live in ps2xRuntime/src/game now (part of the CMake target); drop the copies older versions left here.
foreach ($f in @('gow_overrides.cpp', 'gow_pad2_packet.h', 'gow_gs_replay.h')) {
    Remove-Item -LiteralPath (Join-Path $runner $f) -Force -ErrorAction SilentlyContinue
}

# GOW-Port: the native IOP code needs no game session either. The text of an IRX at its load base is a function of the IRX
# file alone, so the manifest (games/scus-97399/iop.manifest: file, base, size and hash per module, no code) rebuilds the dump
# the game writes with PS2X_IOP_DUMP_MODULES from the disc's own IRX files, and iop-gen compiles it before the build. Without
# this, an installation that skipped the optimization run interpreted every IRX instruction.
$iopManifest = Join-Path $Profile 'iop.manifest'
if (Test-Path -LiteralPath $iopManifest) {
    Step 'IOP modules from the disc (without playing)'
    $iopDisc = Join-Path $Gen 'iop_disc'
    & $toolsExe iop-manifest apply $Game $iopManifest $iopDisc
    if ($LASTEXITCODE -eq 0) {
        Run $toolsExe @('iop-gen', $iopDisc, (Join-Path $Native 'ps2xIOP\src\native'))
    }
    else { Warn 'Could not rebuild the IOP modules from this disc; the IOP code will be captured by the optimization run, or interpreted.' }
}

# --- 3. Game ------------------------------------------------------------------------------------------
function BuildGame([string]$what = '') {
    Step "Building the game$what (the first time takes a while: ~6,400 files; VU1 RAM profile: $Vu1MemoryLimitGB GB)"
    Run 'ninja' @('-C', $Build, "-j$Jobs", 'ps2EntryRunner')
    # GOW-Port: bind the exact local ELF to the generated runner; the launcher checks this fingerprint.
    (Get-Sha256 (Join-Path $Game 'SCUS_973.99')) |
        Set-Content -LiteralPath (Join-Path (Split-Path $GameExe) 'game-elf.sha256') -Encoding ASCII
}
$GameExe = Join-Path $Build 'ps2xRuntime\gow-2005.exe'

# GOW-Port: the optimization run. The game plays a scripted route by itself; it records the VU1 micro-memories that have no
# native code yet and the IOP modules and, in a PGO training build, the profile. It is closed through its window (the profile
# is written then), and only stopped if it does not exit within a minute.
function RunOptimization([string]$profileDir = '') {
    Step "Optimization: the game opens by itself for $Seconds s (do not touch the keyboard); the VU1 and IOP code$(if ($profileDir) { ' and the speed profile are' } else { ' is' }) saved"
    $mods = Join-Path $Gen 'iop_mods'
    New-Item -ItemType Directory -Force -Path $Vu1, $mods | Out-Null
    $vars = @{
        GOW_VU1_CAPTURE = $Vu1; PS2X_IOP_DUMP_MODULES = $mods; GOW_SKIP_FMV = '1'; GOW_PAD_TEST = '1';
        GOW_PAD_TEST_NO_CAPTURE = '1'; GOW_MC0 = (Join-Path $Gen 'optimize.ps2'); PS2X_GS_WORKER = '1'; GOW_GS_FINISH_ASYNC = '1';
        # GOW-Port: Kratos died long before the end of the scripted route and the rest of the run was the game-over menu, so the
        # capture and the speed profile learned menu code. God mode keeps the whole run in the fight.
        PC_CHEATS = 'god_mode';
        GOW_PAD_SCRIPT = '5:start,12:x,20:x,28:x,40:square,41:square,42:square,44:triangle,48:lup*3,52:x,55:square,56:triangle,60:circle,64:r1,70:lleft*2,74:square,76:square,80:triangle,90:lup*4,96:x,100:square,104:l1,110:lright*2,120:square,125:triangle,140:lup*5,150:square,160:triangle,180:lup*5,200:square'
    }
    if ($profileDir) {
        New-Item -ItemType Directory -Force -Path $profileDir | Out-Null
        Get-ChildItem -Path $profileDir -Filter *.profraw -ErrorAction SilentlyContinue | Remove-Item -Force
        $vars['LLVM_PROFILE_FILE'] = Join-Path $profileDir 'gow-%p.profraw'
    }
    foreach ($k in $vars.Keys) { Set-Item -Path "env:$k" -Value $vars[$k] }
    $p = Start-Process -FilePath $GameExe -ArgumentList "`"$(Join-Path $Game 'SCUS_973.99')`"" -WorkingDirectory $Native -PassThru
    # GOW-Port: the game inherited the lowered priority; this run feeds the VU1 and IOP capture, so it must run at normal speed.
    try { $p.PriorityClass = [Diagnostics.ProcessPriorityClass]::Normal } catch { Warn "Could not restore the game's priority: $($_.Exception.Message)" }
    if (-not $p.WaitForExit($Seconds * 1000)) {
        # Only the process this script started, by its PID: first through its window, as a player closes it.
        [void]$p.CloseMainWindow()
        if (-not $p.WaitForExit(60000)) { Stop-Process -Id $p.Id -Force }
    }
    foreach ($k in $vars.Keys) { Remove-Item -Path "env:$k" -ErrorAction SilentlyContinue }
    $vu1Count = (Get-ChildItem -Path $Vu1 -Filter *.bin -ErrorAction SilentlyContinue).Count
    $iopCount = (Get-ChildItem -Path $mods -Filter *.bin -ErrorAction SilentlyContinue).Count
    Write-Host "VU1: $vu1Count micro-memories; IOP: $iopCount modules."
    if ($iopCount -gt 0) { Run $toolsExe @('iop-gen', $mods, (Join-Path $Native 'ps2xIOP\src\native')) }
}

Configure
BuildGame $(if ($trainPgo) { ' with instrumentation for the speed profile' } else { '' })

# --- 4. Optimization (native VU1 and IOP, and the PGO training) --------------------------------------------------
# GOW-Port: the folder the build reads (-Vu1BuildDir) counts too, or a build from captures kept elsewhere opened the game again.
$hasVu1 = [bool](Get-ChildItem -Path @($Vu1, $Vu1ForBuild) -Filter *.bin -ErrorAction SilentlyContinue | Select-Object -First 1)
if ($trainPgo) {
    # One run gives the VU1 / IOP captures and the profile; one more build uses both (gow-native-pc's "tune" stage: preparation
    # with PGO in ~24 minutes instead of ~63 with separate runs).
    RunOptimization $PgoDir
    $raw = @(Get-ChildItem -Path $PgoDir -Filter *.profraw -ErrorAction SilentlyContinue)
    if ($raw.Count -gt 0) {
        Run (Join-Path $ClangBin 'llvm-profdata.exe') (@('merge', '-o', $PgoProfile) + @($raw | ForEach-Object { $_.FullName }))
        $raw | Remove-Item -Force -ErrorAction SilentlyContinue
        $script:PgoMode = 'use'
    }
    else {
        Warn 'The optimization run wrote no speed profile (did the game close early?): the game is built with the profile of the repository instead.'
        $script:PgoMode = if ($hasShipped) { 'shipped' } else { '' }
    }
    Configure
    BuildGame $(if ($script:PgoMode -eq 'use') { ' with the speed profile' } else { '' })
}
else {
    if ($hasVu1 -and -not $Optimize) { Write-Host 'VU1: micro-memories available (manifest or captures); there is no need to open the game to capture.' }
    if ($Optimize -or (-not $NoOptimize -and -not $hasVu1)) {
        RunOptimization
        Configure
        BuildGame
    }
}

Step 'Done'
Write-Host "  Game: $GameExe"
Write-Host '  Open the launcher and use Files > Find files automatically (or run the game from its wizard).'
Write-Host '  More native code after playing new areas: install.cmd -Optimize'
if (-not $NoPause) { Read-Host 'Press Enter to exit' }
