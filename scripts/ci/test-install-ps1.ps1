# Tests of the pure decisions of install.ps1 (the build job count) without running an installation: the functions are taken
# out of the script with the PowerShell parser and defined here, so nothing else in it runs. Works in PowerShell 5.1 and 7.
# Usage: pwsh -NoProfile -File scripts/ci/test-install-ps1.ps1
$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot '..\..\install.ps1'
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $script).Path, [ref]$null, [ref]$null)
$failures = 0

# The text of a function of install.ps1; it is defined here with a dot-source at script level (a function's own scope would lose it).
function Get-ScriptFunction([string]$name) {
    $f = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true)
    if (-not $f) { throw "install.ps1 has no function $name" }
    return $f.Extent.Text
}
function Expect([string]$what, $expected, $actual) {
    if ($expected -ceq $actual) { Write-Host "  ok   $what" }
    else { Write-Host "  FAIL $what (expected '$expected', got '$actual')"; $script:failures++ }
}

. ([scriptblock]::Create((Get-ScriptFunction 'Get-BuildJobs')))
Write-Host 'build jobs: cores, RAM in GB -> jobs'
# (cores, RAM, expected): the reserve is 2 from 8 logical processors, 1 from 4, 0 below; one job per GB above 3 GB or one per
# 2 GB, whichever is more; never below 1.
$jobCases = @(
    @(16, 32, 14), @(8, 32, 6), @(6, 16, 5), @(4, 16, 3), @(2, 16, 2), @(16, 8, 5), @(1, 4, 1),
    @(12, 16, 10), @(7, 64, 6), @(3, 64, 3), @(4, 2, 1), @(8, 1, 1), @(32, 128, 30), @(2, 2, 1),
    @(8, 8, 5), @(4, 4, 2), @(12, 12, 9), @(16, 6, 3)
)
foreach ($c in $jobCases) { Expect "$($c[0]) cores, $($c[1]) GB" $c[2] (Get-BuildJobs $c[0] $c[1]) }

Write-Host 'processor check (A16): the decision with the API result injected'
$requirement = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$CpuRequirement' }, $true)
if (-not $requirement) { throw 'install.ps1 does not define $CpuRequirement' }
. ([scriptblock]::Create($requirement.Extent.Text))
. ([scriptblock]::Create((Get-ScriptFunction 'Get-CpuProblem')))
. ([scriptblock]::Create((Get-ScriptFunction 'Test-Sse41')))
function Warn([string]$t) { Write-Host "  (warning) $t" }
Expect 'SSE4.1 present: nothing to report' $null (Get-CpuProblem $true)
Expect 'the query could not be made: fail open' $null (Get-CpuProblem $null)
$message = Get-CpuProblem $false
Expect 'SSE4.1 absent: the message names SSE4.1' $true ($message -like "*does not support SSE4.1, which the game needs (Intel Core 2 'Penryn' from 2008 or newer, AMD 'Bulldozer' from 2011 or newer). The game cannot run here, so the installation is blocked.")
# The real query on this machine: only its type is asserted (a CI runner or a virtual machine may or may not report the feature);
# $null is allowed (another architecture, or the type could not be added).
$live = Test-Sse41
Expect 'the live query answers true, false or nothing' $true (($null -eq $live) -or ($live -is [bool]))
Write-Host "  info: this machine reports SSE4.1 = $(if ($null -eq $live) { 'unknown' } else { $live })"

Write-Host 'Visual Studio loader (issue #23): the Windows interpreter is not looked up in PATH'
# devkitPro and MSYS2 put a file named "cmd" first in PATH, which PowerShell cannot run in a pipeline: no script may call a bare cmd.
foreach ($name in 'install.ps1', 'scripts\build_release_windows.ps1', 'scripts\build_runtime_windows.ps1') {
    $file = (Resolve-Path (Join-Path $PSScriptRoot "..\..\$name")).Path
    $tree = [System.Management.Automation.Language.Parser]::ParseFile($file, [ref]$null, [ref]$null)
    $bare = @($tree.FindAll({ param($n) $n -is [System.Management.Automation.Language.CommandAst] -and $n.GetCommandName() -in 'cmd', 'cmd.exe' }, $true))
    Expect "$name has no bare cmd call" 0 $bare.Count
}
if ($env:ComSpec -and (Test-Path -LiteralPath $env:ComSpec)) {
    $fake = Join-Path ([IO.Path]::GetTempPath()) "gow-fakecmd-$PID"
    New-Item -ItemType Directory -Path $fake -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $fake 'cmd') -Value 'not an interpreter'
    $loader = Join-Path $fake 'loader.cmd'
    Set-Content -LiteralPath $loader -Value "@set GOW_LOADER_TEST=loaded`r`n@set" -Encoding ASCII
    $savedPath = $env:PATH
    try {
        $env:PATH = "$fake$([IO.Path]::PathSeparator)$env:PATH"
        & $env:ComSpec /c $loader | ForEach-Object { if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "env:$($matches[1])" -Value $matches[2] } }
        Expect 'the loader runs with a fake cmd first in PATH' 'loaded' $env:GOW_LOADER_TEST
    } finally {
        $env:PATH = $savedPath
        Remove-Item Env:GOW_LOADER_TEST -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $fake -Recurse -Force -ErrorAction SilentlyContinue
    }
} else {
    Write-Host '  skip the live loader check (no Windows command interpreter on this system)'
}

# GOW-Port (issue #39): one broken way of downloading (Invoke-WebRequest failing with "The system cannot find the file specified")
# ended the installation for every tool. Every download goes through Fetch, which tries the next way and checks the SHA-256. Run
# here with fake ways, without the network.
Write-Host 'downloads (issue #39): every one goes through Fetch, which falls back and checks the SHA-256'
$downloaders = @($ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.CommandAst] -and
            @('Invoke-WebRequest', 'Start-BitsTransfer', 'iwr', 'curl', 'wget') -contains $n.GetCommandName() }, $true) |
        Where-Object {
            $node = $_.Parent
            while ($node -and -not ($node -is [System.Management.Automation.Language.FunctionDefinitionAst])) { $node = $node.Parent }
            -not $node -or $node.Name -ne 'FetchWith'
        } | ForEach-Object { "line $($_.Extent.StartLineNumber)" })
Expect 'no download outside FetchWith' '' ($downloaders -join ', ')
. ([scriptblock]::Create((Get-ScriptFunction 'FileSha256')))
. ([scriptblock]::Create((Get-ScriptFunction 'Fetch')))
$work = Join-Path ([IO.Path]::GetTempPath()) "gow-fetch-test-$PID"
New-Item -ItemType Directory -Force -Path $work | Out-Null
try {
    $good = Join-Path $work 'good.bin'
    [IO.File]::WriteAllText($good, 'the expected bytes')
    $sha = FileSha256 $good
    $script:tried = @()
    function FetchWith([string]$method, [string]$url, [string]$target) {
        $script:tried += $method
        switch ($method) {
            'Invoke-WebRequest' { throw 'The system cannot find the file specified' }
            'HttpClient' { [IO.File]::WriteAllText($target, 'a captive portal page') }
            default { Copy-Item -LiteralPath $good -Destination $target }
        }
    }
    $target = Join-Path $work 'sub\tool.zip'
    Fetch 'tool' 'https://example.invalid/tool.zip' $target $sha *> $null
    Expect 'a failed and a wrong download are skipped' 'Invoke-WebRequest,HttpClient,curl' ($script:tried -join ',')
    Expect 'the file that arrived is the expected one' $sha (FileSha256 $target)
    $script:tried = @()
    Fetch 'tool' 'https://example.invalid/tool.zip' $target $sha *> $null
    Expect 'a file already in place (saved by hand) is used without downloading' '' ($script:tried -join ',')
    function FetchWith([string]$method, [string]$url, [string]$target) { throw 'offline' }
    [IO.File]::WriteAllText($target, 'a broken file')
    $message = try { Fetch 'tool' 'https://example.invalid/tool.zip' $target $sha *> $null; '' } catch { $_.Exception.Message }
    Expect 'when every way fails the error says where to save the file by hand' $true ($message -like "*save it as $target*")
    Expect 'a wrong file in place is not kept' $false (Test-Path -LiteralPath $target)
}
finally { Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue }

# GOW-Port: PowerShell names ignore case, so `$compiler = ...` inside a function hid the -Compiler parameter and the clang
# build silently used MSVC. No function of install.ps1 may assign a variable named like one of the script's parameters.
Write-Host 'parameters: no function assigns a variable of the same name (names ignore case)'
$parameters = @($ast.ParamBlock.Parameters | ForEach-Object { $_.Name.VariablePath.UserPath })
$shadows = @($ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $true) | ForEach-Object {
        $function = $_
        $function.Body.FindAll({ param($n) $n -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                $n.Left -is [System.Management.Automation.Language.VariableExpressionAst] }, $true) | ForEach-Object {
            $name = $_.Left.VariablePath.UserPath
            if (-not $_.Left.VariablePath.IsScript -and ($parameters | Where-Object { $_ -ieq $name })) { "$($function.Name): `$$name" }
        }
    })
Expect 'no parameter is shadowed' '' ($shadows -join ', ')

if ($failures -gt 0) { Write-Host "$failures test(s) failed"; exit 1 }
Write-Host 'all passed'
