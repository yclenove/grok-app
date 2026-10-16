// Disposable CI launcher only. Production engines still reject elevated tokens.
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace Gcu
{
    public sealed class DesktopFacts
    {
        public int SessionId;
        public bool IsAdmin;
        public string IntegrityLevel;
        public int IntegrityRid;
        public bool UserInteractive;
        public string Desktop;
    }

    public static class DesktopLauncher
    {
        const uint TOKEN_ALL_ACCESS = 0x000f01ff;
        const uint LUA_TOKEN = 0x4, DISABLE_MAX_PRIVILEGE = 0x1;
        const int TokenSessionId = 12, TokenIntegrityLevel = 25;
        const uint CREATE_NEW_CONSOLE = 0x10;

        [StructLayout(LayoutKind.Sequential)]
        struct SidAndAttributes { public IntPtr Sid; public uint Attributes; }
        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        struct StartupInfo
        {
            public uint cb;
            public string lpReserved, lpDesktop, lpTitle;
            public uint dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
            public ushort wShowWindow, cbReserved2;
            public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
        }
        [StructLayout(LayoutKind.Sequential)]
        struct ProcessInfo { public IntPtr hProcess, hThread; public uint dwProcessId, dwThreadId; }

        [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
        [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool CloseHandle(IntPtr handle);
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool CreateRestrictedToken(IntPtr existing, uint flags, uint disableCount,
            [In] SidAndAttributes[] disable, uint privilegeCount, IntPtr privileges,
            uint restrictedCount, IntPtr restricted, out IntPtr token);
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool GetTokenInformation(IntPtr token, int kind, IntPtr buffer, uint length, out uint needed);
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool SetTokenInformation(IntPtr token, int kind, ref SidAndAttributes label, uint length);
        [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool ConvertStringSidToSidW(string value, out IntPtr sid);
        [DllImport("advapi32.dll")] static extern uint GetLengthSid(IntPtr sid);
        [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr memory);
        [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool CreateProcessAsUserW(IntPtr token, string application, StringBuilder command,
            IntPtr processAttributes, IntPtr threadAttributes, [MarshalAs(UnmanagedType.Bool)] bool inheritHandles,
            uint flags, IntPtr environment, string directory, ref StartupInfo startup, out ProcessInfo info);
        [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool CreateProcessWithTokenW(IntPtr token, uint logonFlags, string application,
            StringBuilder command, uint flags, IntPtr environment, string directory,
            ref StartupInfo startup, out ProcessInfo info);
        [DllImport("user32.dll")] static extern IntPtr GetProcessWindowStation();
        [DllImport("user32.dll", SetLastError = true)] static extern IntPtr GetThreadDesktop(uint thread);
        [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool GetUserObjectInformationW(IntPtr handle, int kind, StringBuilder value, uint length, out uint needed);

        static Win32Exception Failure(string operation, int error)
        {
            return new Win32Exception(error, operation + ": Win32=" + error + " (" + new Win32Exception(error).Message + ")");
        }
        static void Check(bool success, string operation)
        {
            if (!success) throw Failure(operation, Marshal.GetLastWin32Error());
        }
        static IntPtr TokenInfo(IntPtr token, int kind)
        {
            uint needed;
            GetTokenInformation(token, kind, IntPtr.Zero, 0, out needed);
            if (needed == 0) throw Failure("GetTokenInformation size", Marshal.GetLastWin32Error());
            IntPtr buffer = Marshal.AllocHGlobal(checked((int)needed));
            try
            {
                Check(GetTokenInformation(token, kind, buffer, needed, out needed), "GetTokenInformation");
                return buffer;
            }
            catch { Marshal.FreeHGlobal(buffer); throw; }
        }
        static int Session(IntPtr token)
        {
            IntPtr buffer = TokenInfo(token, TokenSessionId);
            try { return Marshal.ReadInt32(buffer); }
            finally { Marshal.FreeHGlobal(buffer); }
        }
        static string ObjectName(IntPtr handle)
        {
            uint needed;
            GetUserObjectInformationW(handle, 2, null, 0, out needed); // UOI_NAME, bytes including NUL
            if (needed == 0) throw Failure("desktop object name size", Marshal.GetLastWin32Error());
            StringBuilder name = new StringBuilder(checked((int)needed / 2));
            Check(GetUserObjectInformationW(handle, 2, name, needed, out needed), "desktop object name");
            return name.ToString();
        }
        public static DesktopFacts ReadFacts()
        {
            IntPtr token;
            Check(OpenProcessToken(GetCurrentProcess(), 0x8, out token), "OpenProcessToken query");
            try
            {
                string sid;
                IntPtr buffer = TokenInfo(token, TokenIntegrityLevel);
                try
                {
                    SidAndAttributes label = (SidAndAttributes)Marshal.PtrToStructure(buffer, typeof(SidAndAttributes));
                    sid = new SecurityIdentifier(label.Sid).Value;
                }
                finally { Marshal.FreeHGlobal(buffer); }
                int rid = int.Parse(sid.Substring(sid.LastIndexOf('-') + 1), CultureInfo.InvariantCulture);
                bool admin;
                using (WindowsIdentity identity = WindowsIdentity.GetCurrent())
                    admin = new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator);
                return new DesktopFacts {
                    SessionId = Session(token), IsAdmin = admin, IntegrityRid = rid,
                    IntegrityLevel = (rid == 8192 ? "Medium" : rid == 12288 ? "High" : "Other") + " (" + sid + ")",
                    UserInteractive = Environment.UserInteractive,
                    Desktop = ObjectName(GetProcessWindowStation()) + "\\" + ObjectName(GetThreadDesktop(GetCurrentThreadId()))
                };
            }
            finally { CloseHandle(token); }
        }

        // Windows argv quoting, also covering paths ending in a backslash.
        public static string QuoteArgument(string value)
        {
            StringBuilder result = new StringBuilder("\"");
            int slashes = 0;
            foreach (char c in value)
            {
                if (c == '\\') { slashes++; continue; }
                result.Append('\\', c == '"' ? slashes * 2 + 1 : slashes);
                result.Append(c);
                slashes = 0;
            }
            return result.Append('\\', slashes * 2).Append('"').ToString();
        }
        public static void CheckLayout()
        {
            // Fail before calling Win32 if Add-Type marshalling differs from the ABI.
            if (Marshal.SizeOf(typeof(StartupInfo)) != (IntPtr.Size == 8 ? 104 : 68) ||
                Marshal.SizeOf(typeof(SidAndAttributes)) != (IntPtr.Size == 8 ? 16 : 8) ||
                Marshal.SizeOf(typeof(ProcessInfo)) != (IntPtr.Size == 8 ? 24 : 16))
                throw new InvalidOperationException("unexpected Win32 structure layout");
        }
        public static Child Start(string executable, string[] arguments, int expectedSession)
        {
            CheckLayout();
            IntPtr source = IntPtr.Zero, restricted = IntPtr.Zero, admins = IntPtr.Zero, medium = IntPtr.Zero;
            try
            {
                Check(OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, out source), "OpenProcessToken launch");
                Check(ConvertStringSidToSidW("S-1-5-32-544", out admins), "Administrators SID");
                SidAndAttributes[] disable = { new SidAndAttributes { Sid = admins } };
                Check(CreateRestrictedToken(source, LUA_TOKEN | DISABLE_MAX_PRIVILEGE, 1, disable,
                    0, IntPtr.Zero, 0, IntPtr.Zero, out restricted), "CreateRestrictedToken");
                Check(ConvertStringSidToSidW("S-1-16-8192", out medium), "Medium integrity SID");
                SidAndAttributes label = new SidAndAttributes { Sid = medium, Attributes = 0x20 }; // SE_GROUP_INTEGRITY
                Check(SetTokenInformation(restricted, TokenIntegrityLevel, ref label,
                    checked((uint)Marshal.SizeOf(typeof(SidAndAttributes)) + GetLengthSid(medium))), "SetTokenInformation Medium");
                if (expectedSession <= 0 || Session(source) != expectedSession || Session(restricted) != expectedSession)
                    throw new InvalidOperationException("restricted token is not in the parent's interactive session");
                string command = QuoteArgument(executable);
                foreach (string argument in arguments) command += " " + QuoteArgument(argument);
                StartupInfo startup = new StartupInfo {
                    cb = (uint)Marshal.SizeOf(typeof(StartupInfo)), lpDesktop = @"WinSta0\Default",
                    dwFlags = 1, wShowWindow = 0 // STARTF_USESHOWWINDOW / SW_HIDE: don't focus the harness console
                };
                ProcessInfo info;
                // Same user's restricted primary token inherits the current environment.
                // No handles, credentials, profile loading, or UAC settings are changed.
                bool created = CreateProcessAsUserW(restricted, executable, new StringBuilder(command),
                    IntPtr.Zero, IntPtr.Zero, false, CREATE_NEW_CONSOLE, IntPtr.Zero, null, ref startup, out info);
                if (!created)
                {
                    int asUserError = Marshal.GetLastWin32Error();
                    // A failed create has no child/effect to replay. This API uses
                    // SeImpersonatePrivilege when AssignPrimaryToken/IncreaseQuota is unavailable.
                    created = CreateProcessWithTokenW(restricted, 0, executable, new StringBuilder(command),
                        CREATE_NEW_CONSOLE, IntPtr.Zero, null, ref startup, out info);
                    if (!created) throw Failure("CreateProcessWithTokenW (CreateProcessAsUserW error " + asUserError + ")",
                        Marshal.GetLastWin32Error());
                }
                CloseHandle(info.hThread);
                return new Child(info.hProcess, info.dwProcessId);
            }
            finally
            {
                if (medium != IntPtr.Zero) LocalFree(medium);
                if (admins != IntPtr.Zero) LocalFree(admins);
                if (restricted != IntPtr.Zero) CloseHandle(restricted);
                if (source != IntPtr.Zero) CloseHandle(source);
            }
        }

        public sealed class Child : IDisposable
        {
            readonly SafeWaitHandle handle;
            public uint ProcessId { get; private set; }
            internal Child(IntPtr process, uint id) { handle = new SafeWaitHandle(process, true); ProcessId = id; }
            [DllImport("kernel32.dll", SetLastError = true)] static extern uint WaitForSingleObject(SafeWaitHandle process, uint ms);
            [DllImport("kernel32.dll", SetLastError = true)]
            [return: MarshalAs(UnmanagedType.Bool)] static extern bool GetExitCodeProcess(SafeWaitHandle process, out uint code);
            [DllImport("kernel32.dll", SetLastError = true)]
            [return: MarshalAs(UnmanagedType.Bool)] static extern bool TerminateProcess(SafeWaitHandle process, uint code);
            public bool HasExited
            {
                get {
                    uint status = WaitForSingleObject(handle, 0);
                    if (status == 0xffffffff) throw Failure("child wait", Marshal.GetLastWin32Error());
                    return status == 0;
                }
            }
            public uint ExitCode { get { uint code; Check(GetExitCodeProcess(handle, out code), "child exit code"); return code; } }
            public void Stop()
            {
                if (HasExited || TerminateProcess(handle, 1)) return;
                int error = Marshal.GetLastWin32Error();
                // Exit may race the zero-time wait; an already dead child needs no kill.
                if (!HasExited) throw Failure("terminate desktop child", error);
            }
            public void Dispose() { handle.Dispose(); }
        }
    }
}
