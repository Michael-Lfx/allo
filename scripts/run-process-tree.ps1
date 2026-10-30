param(
  [Parameter(Mandatory = $true)]
  [string]$Executable,
  [Parameter(Mandatory = $true)]
  [string]$ArgsJsonBase64,
  [string]$WorkingDirectory = $null,
  [string]$EnvJsonBase64 = $null,
  [int]$TimeoutMs = 0
)

Add-Type @'
using System;
using System.IO;
using System.Text;
using System.Threading.Tasks;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

public static class FlowyProcessTreeJob {
    private const uint JobObjectExtendedLimitInformation = 9;
    private const uint JobObjectLimitKillOnJobClose = 0x2000;
    private const uint CREATE_SUSPENDED = 0x00000004;
    private const uint CREATE_NO_WINDOW = 0x08000000;
    private const uint STARTF_USESTDHANDLES = 0x00000100;
    private const uint HANDLE_FLAG_INHERIT = 0x00000001;
    private const uint WAIT_OBJECT_0 = 0x00000000;
    private const uint WAIT_TIMEOUT = 0x00000102;
    private const uint INFINITE = 0xFFFFFFFF;

    [StructLayout(LayoutKind.Sequential)]
    public struct PROCESS_INFORMATION {
        public IntPtr hProcess;
        public IntPtr hThread;
        public uint dwProcessId;
        public uint dwThreadId;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct STARTUPINFO {
        public uint cb;
        public string lpReserved;
        public string lpDesktop;
        public string lpTitle;
        public uint dwX;
        public uint dwY;
        public uint dwXSize;
        public uint dwYSize;
        public uint dwXCountChars;
        public uint dwYCountChars;
        public uint dwFillAttribute;
        public uint dwFlags;
        public ushort wShowWindow;
        public ushort cbReserved2;
        public IntPtr lpReserved2;
        public IntPtr hStdInput;
        public IntPtr hStdOutput;
        public IntPtr hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct SECURITY_ATTRIBUTES {
        public uint nLength;
        public IntPtr lpSecurityDescriptor;
        [MarshalAs(UnmanagedType.Bool)]
        public bool bInheritHandle;
    }

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
    public static extern bool TerminateJobObject(IntPtr job, uint exitCode);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern bool CreateProcessW(
        string lpApplicationName,
        string lpCommandLine,
        ref SECURITY_ATTRIBUTES lpProcessAttributes,
        ref SECURITY_ATTRIBUTES lpThreadAttributes,
        [MarshalAs(UnmanagedType.Bool)] bool bInheritHandles,
        uint dwCreationFlags,
        IntPtr lpEnvironment,
        string lpCurrentDirectory,
        ref STARTUPINFO lpStartupInfo,
        out PROCESS_INFORMATION lpProcessInformation
    );

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint ResumeThread(IntPtr hThread);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool TerminateProcess(IntPtr hProcess, uint uExitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint WaitForSingleObject(IntPtr hHandle, uint dwMilliseconds);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetExitCodeProcess(IntPtr hProcess, out uint lpExitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CreatePipe(
        out IntPtr hReadPipe,
        out IntPtr hWritePipe,
        ref SECURITY_ATTRIBUTES lpPipeAttributes,
        uint nSize
    );

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetHandleInformation(IntPtr hObject, uint dwMask, uint dwFlags);

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

    public static int RunProcessInJob(
        string executable,
        string commandLine,
        string workingDirectory,
        int timeoutMs,
        out string stdout,
        out string stderr,
        out bool timedOut
    ) {
        timedOut = false;
        stdout = "";
        stderr = "";

        var job = CreateKillOnCloseJob();
        IntPtr hStdoutRead = IntPtr.Zero;
        IntPtr hStdoutWrite = IntPtr.Zero;
        IntPtr hStderrRead = IntPtr.Zero;
        IntPtr hStderrWrite = IntPtr.Zero;

        PROCESS_INFORMATION pi = new PROCESS_INFORMATION();

        try {
            var sa = new SECURITY_ATTRIBUTES();
            sa.nLength = (uint)Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES));
            sa.bInheritHandle = true;

            if (!CreatePipe(out hStdoutRead, out hStdoutWrite, ref sa, 0)) {
                throw new System.ComponentModel.Win32Exception();
            }
            if (!SetHandleInformation(hStdoutRead, HANDLE_FLAG_INHERIT, 0)) {
                throw new System.ComponentModel.Win32Exception();
            }

            if (!CreatePipe(out hStderrRead, out hStderrWrite, ref sa, 0)) {
                throw new System.ComponentModel.Win32Exception();
            }
            if (!SetHandleInformation(hStderrRead, HANDLE_FLAG_INHERIT, 0)) {
                throw new System.ComponentModel.Win32Exception();
            }

            var si = new STARTUPINFO();
            si.cb = (uint)Marshal.SizeOf(typeof(STARTUPINFO));
            si.dwFlags = STARTF_USESTDHANDLES;
            si.hStdOutput = hStdoutWrite;
            si.hStdError = hStderrWrite;

            var threadSa = new SECURITY_ATTRIBUTES();
            threadSa.nLength = (uint)Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES));
            threadSa.bInheritHandle = false;

            // Step 1: Create the process suspended so it CANNOT execute before being added to the Job!
            uint creationFlags = CREATE_SUSPENDED | CREATE_NO_WINDOW;

            if (!CreateProcessW(
                null,
                commandLine,
                ref sa,
                ref threadSa,
                true,
                creationFlags,
                IntPtr.Zero,
                string.IsNullOrEmpty(workingDirectory) ? null : workingDirectory,
                ref si,
                out pi
            )) {
                throw new System.ComponentModel.Win32Exception();
            }

            // Close write pipe ends in parent process so readers observe EOF on child process termination
            CloseHandle(hStdoutWrite); hStdoutWrite = IntPtr.Zero;
            CloseHandle(hStderrWrite); hStderrWrite = IntPtr.Zero;

            // Step 2: Assign the suspended process to the Job Object.
            // If this fails, the process is terminated while still suspended: ZERO race condition window!
            if (!AssignProcessToJobObject(job, pi.hProcess)) {
                int error = Marshal.GetLastWin32Error();
                TerminateProcess(pi.hProcess, 1);
                throw new System.ComponentModel.Win32Exception(error);
            }

            // Step 3: Process is guaranteed in Job Object. Resume its primary thread!
            if (ResumeThread(pi.hThread) == unchecked((uint)-1)) {
                int error = Marshal.GetLastWin32Error();
                TerminateProcess(pi.hProcess, 1);
                throw new System.ComponentModel.Win32Exception(error);
            }

            // Read output streams asynchronously
            var outStream = new FileStream(new SafeFileHandle(hStdoutRead, true), FileAccess.Read);
            hStdoutRead = IntPtr.Zero;
            var errStream = new FileStream(new SafeFileHandle(hStderrRead, true), FileAccess.Read);
            hStderrRead = IntPtr.Zero;

            var outTask = Task.Run(() => {
                using (var reader = new StreamReader(outStream, Encoding.UTF8)) {
                    return reader.ReadToEnd();
                }
            });
            var errTask = Task.Run(() => {
                using (var reader = new StreamReader(errStream, Encoding.UTF8)) {
                    return reader.ReadToEnd();
                }
            });

            uint waitTime = timeoutMs > 0 ? (uint)timeoutMs : INFINITE;
            uint waitRes = WaitForSingleObject(pi.hProcess, waitTime);

            if (waitRes == WAIT_TIMEOUT) {
                timedOut = true;
                // Forcefully terminate the ENTIRE Job Object (process + all descendant children)
                TerminateJobObject(job, 124);
                WaitForSingleObject(pi.hProcess, 3000);
                try { Task.WaitAll(new Task[] { outTask, errTask }, 2000); } catch {}
                stdout = outTask.IsCompleted ? outTask.Result : "";
                stderr = errTask.IsCompleted ? errTask.Result : "";
                return 124;
            }

            uint exitCode = 0;
            GetExitCodeProcess(pi.hProcess, out exitCode);
            try { Task.WaitAll(new Task[] { outTask, errTask }, 5000); } catch {}
            stdout = outTask.IsCompleted ? outTask.Result : "";
            stderr = errTask.IsCompleted ? errTask.Result : "";
            return (int)exitCode;
        } finally {
            if (hStdoutWrite != IntPtr.Zero) CloseHandle(hStdoutWrite);
            if (hStderrWrite != IntPtr.Zero) CloseHandle(hStderrWrite);
            if (hStdoutRead != IntPtr.Zero) CloseHandle(hStdoutRead);
            if (hStderrRead != IntPtr.Zero) CloseHandle(hStderrRead);

            if (pi.hThread != IntPtr.Zero) CloseHandle(pi.hThread);
            if (pi.hProcess != IntPtr.Zero) {
                if (timedOut) {
                    try { TerminateJobObject(job, 124); } catch {}
                }
                CloseHandle(pi.hProcess);
            }
            if (job != IntPtr.Zero) {
                try { TerminateJobObject(job, 1); } catch {}
                CloseHandle(job);
            }
        }
    }
}
'@

function ConvertTo-WindowsArgument {
  param([AllowEmptyString()][string]$Value)
  if ($Value.Length -eq 0) { return '""' }
  if ($Value -notmatch '[\s"]') { return $Value }
  $escaped = $Value -replace '(\\+)"', '$1$1\"'
  $escaped = $escaped -replace '(\\+)$', '$1$1'
  return '"' + $escaped + '"'
}

$argumentsJson = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($ArgsJsonBase64))
$parsedArguments = $argumentsJson | ConvertFrom-Json
$arguments = @()
foreach ($argument in $parsedArguments) {
  $arguments += [string]$argument
}

# Apply environment overrides if provided
if ($EnvJsonBase64) {
  $envJson = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($EnvJsonBase64))
  $envHash = $envJson | ConvertFrom-Json
  foreach ($prop in $envHash.psobject.Properties) {
    [Environment]::SetEnvironmentVariable($prop.Name, [string]$prop.Value, 'Process')
  }
}

$commandLine = (ConvertTo-WindowsArgument $Executable) + ' ' + (($arguments | ForEach-Object { ConvertTo-WindowsArgument ([string]$_) }) -join ' ')
$stdout = ""
$stderr = ""
$timedOut = $false

$exitCode = [FlowyProcessTreeJob]::RunProcessInJob(
  $Executable,
  $commandLine,
  $WorkingDirectory,
  $TimeoutMs,
  [ref]$stdout,
  [ref]$stderr,
  [ref]$timedOut
)

if ($stdout.Length -gt 0) {
  [Console]::Out.Write($stdout)
}
if ($stderr.Length -gt 0) {
  [Console]::Error.Write($stderr)
}
if ($timedOut) {
  [Console]::Error.WriteLine("PROCESS_TIMEOUT: Process tree exceeded $TimeoutMs ms and was terminated")
}

exit $exitCode
