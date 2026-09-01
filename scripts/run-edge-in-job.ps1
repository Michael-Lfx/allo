param(
  [Parameter(Mandatory = $true)]
  [string]$Executable,
  [Parameter(Mandatory = $true)]
  [string]$ArgsJsonBase64
)

# Keep the browser and every renderer/utility process in an owned Windows Job.
# JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE makes the PowerShell launcher the lifetime
# owner: when it exits or is task-killed, Edge descendants are terminated too.
Add-Type @'
using System;
using System.Runtime.InteropServices;

public static class FlowyEdgeJob {
    private const uint JobObjectExtendedLimitInformation = 9;
    private const uint JobObjectLimitKillOnJobClose = 0x2000;

    [StructLayout(LayoutKind.Sequential)]
    private struct BasicLimitInformation {
        public long PerProcessUserTimeLimit;
        public long PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize;
        public UIntPtr MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass;
        public uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct IoCounters {
        public ulong ReadOperationCount;
        public ulong WriteOperationCount;
        public ulong OtherOperationCount;
        public ulong ReadTransferCount;
        public ulong WriteTransferCount;
        public ulong OtherTransferCount;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ExtendedLimitInformation {
        public BasicLimitInformation BasicLimitInformation;
        public IoCounters IoInfo;
        public UIntPtr ProcessMemoryLimit;
        public UIntPtr JobMemoryLimit;
        public UIntPtr PeakProcessMemoryUsed;
        public UIntPtr PeakJobMemoryUsed;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateJobObject(IntPtr attributes, string name);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetInformationJobObject(
        IntPtr job,
        uint informationClass,
        IntPtr information,
        uint informationLength
    );

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr handle);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool TerminateJobObject(IntPtr job, uint exitCode);

    public static IntPtr CreateKillOnCloseJob() {
        var job = CreateJobObject(IntPtr.Zero, null);
        if (job == IntPtr.Zero) throw new System.ComponentModel.Win32Exception();

        var info = new ExtendedLimitInformation();
        info.BasicLimitInformation.LimitFlags = JobObjectLimitKillOnJobClose;
        var size = Marshal.SizeOf(typeof(ExtendedLimitInformation));
        var buffer = Marshal.AllocHGlobal(size);
        try {
            Marshal.StructureToPtr(info, buffer, false);
            if (!SetInformationJobObject(job, JobObjectExtendedLimitInformation, buffer, (uint)size)) {
                throw new System.ComponentModel.Win32Exception();
            }
            return job;
        } catch {
            CloseHandle(job);
            throw;
        } finally {
            Marshal.FreeHGlobal(buffer);
        }
    }

    public static void Assign(IntPtr job, IntPtr process) {
        if (!AssignProcessToJobObject(job, process)) {
            throw new System.ComponentModel.Win32Exception();
        }
    }

    public static void Terminate(IntPtr job) {
        if (!TerminateJobObject(job, 1)) {
            var error = Marshal.GetLastWin32Error();
            if (error != 5 && error != 87) {
                throw new System.ComponentModel.Win32Exception(error);
            }
        }
    }
}
'@

function ConvertTo-WindowsArgument {
  param([AllowEmptyString()][string]$Value)
  if ($Value.Length -eq 0) { return '""' }
  if ($Value -notmatch '[\s"]') { return $Value }
  $escaped = $Value -replace '(\\*)"', '$1$1\"'
  $escaped = $escaped -replace '(\\+)$', '$1$1'
  return '"' + $escaped + '"'
}

$argumentsJson = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($ArgsJsonBase64))
$parsedArguments = $argumentsJson | ConvertFrom-Json
$arguments = @()
foreach ($argument in $parsedArguments) {
  $arguments += [string]$argument
}
$job = [FlowyEdgeJob]::CreateKillOnCloseJob()
$process = $null

try {
  $startInfo = New-Object System.Diagnostics.ProcessStartInfo
  $startInfo.FileName = $Executable
  $startInfo.Arguments = (($arguments | ForEach-Object { ConvertTo-WindowsArgument ([string]$_) }) -join ' ')
  $startInfo.UseShellExecute = $false
  $startInfo.CreateNoWindow = $true
  $startInfo.RedirectStandardOutput = $true
  $startInfo.RedirectStandardError = $true

  $process = New-Object System.Diagnostics.Process
  $process.StartInfo = $startInfo
  if (-not $process.Start()) { throw "Could not start Edge: $Executable" }
  try {
    [FlowyEdgeJob]::Assign($job, $process.Handle)
  } catch {
    try { $process.Kill() } catch { }
    try { & taskkill.exe /PID $process.Id /T /F 2>$null | Out-Null } catch { }
    throw
  }

  $stdoutTask = $process.StandardOutput.ReadToEndAsync()
  $stderrTask = $process.StandardError.ReadToEndAsync()
  $process.WaitForExit()
  [Console]::Out.Write($stdoutTask.Result)
  [Console]::Error.Write($stderrTask.Result)
  exit $process.ExitCode
} finally {
  if ($process -and -not $process.HasExited) {
    try { $process.Kill() } catch { }
    try { & taskkill.exe /PID $process.Id /T /F 2>$null | Out-Null } catch { }
  }
  try { [FlowyEdgeJob]::Terminate($job) } catch { }
  [FlowyEdgeJob]::CloseHandle($job) | Out-Null
}
