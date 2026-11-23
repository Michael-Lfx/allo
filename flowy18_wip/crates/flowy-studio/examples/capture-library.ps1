param(
    [Parameter(Mandatory = $true)]
    [string]$ExampleExecutable,
    [string]$OutputDirectory,
    [ValidateRange(10, 60)]
    [int]$ProcessTimeoutSeconds = 30
)

$ErrorActionPreference = 'Stop'
$taskRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$taskExecutable = (Resolve-Path -LiteralPath $ExampleExecutable).Path
if (-not (Test-Path -LiteralPath $taskExecutable -PathType Leaf)) {
    throw 'Pass the path to an already-built library example executable.'
}
if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path $taskRoot '.scratch\conversation-artifacts-native'
}
$taskOutput = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $taskOutput | Out-Null

# These inputs belong to the isolated example only. No app data directory,
# conversation history, production seed, or external application is used.
$taskCases = @(
    @{ Name = '01-library-zh-light-large'; Small = $false; Arguments = @(); Delay = 1500 },
    @{ Name = '02-library-en-dark-small'; Small = $true; Arguments = @('--en', '--dark'); Delay = 1500 },
    @{ Name = '03-empty-zh-light-small'; Small = $true; Arguments = @('--empty'); Delay = 1500 },
    @{ Name = '04-filter-empty-en-dark-small'; Small = $true; Arguments = @('--en', '--dark', '--search-empty'); Delay = 1500 },
    @{ Name = '05-trash-zh-dark-large'; Small = $false; Arguments = @('--trash', '--dark'); Delay = 1500 },
    @{ Name = '06-catalog-error-en-light-small'; Small = $true; Arguments = @('--en', '--error'); Delay = 1500 },
    @{ Name = '07-loading-zh-dark-small'; Small = $true; Arguments = @('--loading', '--dark'); Delay = 1500 },
    @{ Name = '08-pdf-en-light-large'; Small = $false; Arguments = @('--en', '--file', 'crates/flowy-studio/examples/fixtures/preview/sample.pdf', '--preview'); Delay = 5000 },
    @{ Name = '09-pptx-zh-dark-small'; Small = $true; Arguments = @('--dark', '--file', 'crates/flowy-studio/examples/fixtures/preview/sample.pptx', '--preview'); Delay = 7000 }
)
$taskEnvironmentNames = @(
    'FLOWY_LIBRARY_CAPTURE',
    'FLOWY_LIBRARY_CAPTURE_EXIT',
    'FLOWY_LIBRARY_CAPTURE_DELAY_MS',
    'FLOWY_LIBRARY_SMALL'
)
$taskPreviousEnvironment = @{}
foreach ($taskName in $taskEnvironmentNames) {
    $taskPreviousEnvironment[$taskName] = [Environment]::GetEnvironmentVariable($taskName, 'Process')
}
$taskResults = @()
try {
    foreach ($taskCase in $taskCases) {
        $taskPng = Join-Path $taskOutput ($taskCase.Name + '.png')
        $taskStdout = Join-Path $taskOutput ($taskCase.Name + '.stdout.log')
        $taskStderr = Join-Path $taskOutput ($taskCase.Name + '.stderr.log')
        if (Test-Path -LiteralPath $taskPng) {
            throw "Capture already exists; use a fresh output folder: $taskPng"
        }
        [Environment]::SetEnvironmentVariable('FLOWY_LIBRARY_CAPTURE', $taskPng, 'Process')
        [Environment]::SetEnvironmentVariable('FLOWY_LIBRARY_CAPTURE_EXIT', '1', 'Process')
        [Environment]::SetEnvironmentVariable('FLOWY_LIBRARY_CAPTURE_DELAY_MS', $taskCase.Delay.ToString(), 'Process')
        [Environment]::SetEnvironmentVariable('FLOWY_LIBRARY_SMALL', $(if ($taskCase.Small) { '1' } else { $null }), 'Process')
        $taskStartArguments = @{
            FilePath = $taskExecutable
            WorkingDirectory = $taskRoot
            WindowStyle = 'Hidden'
            PassThru = $true
            RedirectStandardOutput = $taskStdout
            RedirectStandardError = $taskStderr
        }
        if ($taskCase.Arguments.Count -gt 0) {
            $taskStartArguments.ArgumentList = $taskCase.Arguments
        }
        $taskStartedAt = [DateTimeOffset]::Now
        $taskProcess = Start-Process @taskStartArguments
        $taskFinished = $taskProcess.WaitForExit($ProcessTimeoutSeconds * 1000)
        if (-not $taskFinished) {
            # Only stop the exact example child launched by this script.
            Stop-Process -Id $taskProcess.Id -ErrorAction SilentlyContinue
            $taskProcess.WaitForExit()
        }
        $taskProcess.Refresh()
        $taskResult = [ordered]@{
            name = $taskCase.Name
            arguments = $taskCase.Arguments
            logical_window = $(if ($taskCase.Small) { '800x600' } else { '1584x982' })
            started_at = $taskStartedAt.ToString('o')
            elapsed_ms = [int]([DateTimeOffset]::Now - $taskStartedAt).TotalMilliseconds
            exited = $taskFinished
            exit_code = $taskProcess.ExitCode
            png = $taskPng
            captured = (Test-Path -LiteralPath $taskPng -PathType Leaf)
            stderr = $taskStderr
        }
        $taskResults += $taskResult
        $taskResult | ConvertTo-Json -Compress | Write-Output
    }
} finally {
    foreach ($taskName in $taskEnvironmentNames) {
        [Environment]::SetEnvironmentVariable($taskName, $taskPreviousEnvironment[$taskName], 'Process')
    }
    $taskReport = [ordered]@{
        executable = $taskExecutable
        workspace = $taskRoot
        scope = 'Isolated native rendering only; not a production delivery or external-app acceptance test.'
        captures = $taskResults
    }
    $taskReport | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $taskOutput 'capture-report.json') -Encoding UTF8
}

if ($taskResults.Count -ne $taskCases.Count -or @($taskResults | Where-Object { -not $_.captured }).Count -gt 0) {
    throw 'One or more native captures failed. Inspect capture-report.json and the per-state stderr logs.'
}
