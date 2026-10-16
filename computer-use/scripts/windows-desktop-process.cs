// CI-only supervisor: contain a complete stage (including blocked Win32/COM
// calls) in a kill-on-close job. Never wait for stdout/stderr pipe EOF.
using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Gcu
{
    public static partial class DesktopLauncher
    {
        [StructLayout(LayoutKind.Sequential)]
        struct JobLimits
        {
            public long PerProcessTime, PerJobTime;
            public uint Flags;
            public UIntPtr MinWorkingSet, MaxWorkingSet;
            public uint ActiveProcesses;
            public UIntPtr Affinity;
            public uint Priority, Scheduling;
        }
        [StructLayout(LayoutKind.Sequential)]
        struct IoCounters { public ulong ReadOps, WriteOps, OtherOps, ReadBytes, WriteBytes, OtherBytes; }
        [StructLayout(LayoutKind.Sequential)]
        struct ExtendedJobLimits
        {
            public JobLimits Basic;
            public IoCounters Io;
            public UIntPtr ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory;
        }
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool SetInformationJobObject(IntPtr job, int kind, ref ExtendedJobLimits limits, uint size);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
        [DllImport("kernel32.dll", SetLastError = true)]
        static extern uint ResumeThread(IntPtr thread);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool CreateProcessW(string application, StringBuilder command, IntPtr processAttributes,
            IntPtr threadAttributes, [MarshalAs(UnmanagedType.Bool)] bool inheritHandles, uint flags,
            IntPtr environment, string directory, ref StartupInfo startup, out ProcessInfo info);
        static void CheckJobLayout()
        {
            if (Marshal.SizeOf(typeof(ExtendedJobLimits)) != (IntPtr.Size == 8 ? 144 : 112))
                throw new InvalidOperationException("unexpected job structure layout");
        }
        public static Child StartSupervisor(string executable, string[] arguments, string directory)
        {
            CheckLayout();
            string command = QuoteArgument(executable);
            foreach (string argument in arguments) command += " " + QuoteArgument(argument);
            ProcessInfo info;
            StartupInfo startup = new StartupInfo {
                cb = (uint)Marshal.SizeOf(typeof(StartupInfo)), lpDesktop = @"WinSta0\Default",
                dwFlags = 1, wShowWindow = 0
            };
            Check(CreateProcessW(executable, new StringBuilder(command), IntPtr.Zero, IntPtr.Zero,
                false, CREATE_NO_WINDOW | 4, IntPtr.Zero, directory, ref startup, out info), "create stage supervisor");
            return ContainAndResume(info);
        }
        static Child ContainAndResume(ProcessInfo info)
        {
            Child child = new Child(info.hProcess, info.dwProcessId);
            IntPtr job = IntPtr.Zero;
            try
            {
                job = CreateJobObjectW(IntPtr.Zero, null);
                if (job == IntPtr.Zero) throw Failure("CreateJobObject", Marshal.GetLastWin32Error());
                ExtendedJobLimits limits = new ExtendedJobLimits();
                limits.Basic.Flags = 0x2000; // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                Check(SetInformationJobObject(job, 9, ref limits, (uint)Marshal.SizeOf(limits)), "job kill-on-close");
                // All launch APIs create suspended. Explicitly assign even the
                // WithToken child (secondary logon may not inherit our outer job).
                Check(AssignProcessToJobObject(job, info.hProcess), "assign process tree job");
                child.OwnJob(job);
                job = IntPtr.Zero;
                if (ResumeThread(info.hThread) == 0xffffffff) throw Failure("resume contained child", Marshal.GetLastWin32Error());
                return child;
            }
            catch
            {
                try { child.Stop(); } finally { child.Dispose(); }
                throw;
            }
            finally
            {
                CloseHandle(info.hThread);
                if (job != IntPtr.Zero) CloseHandle(job);
            }
        }
    }
}
