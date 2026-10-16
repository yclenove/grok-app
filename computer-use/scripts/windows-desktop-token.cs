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
        public string TokenSummary;
    }

    public static partial class DesktopLauncher
    {
        const uint TOKEN_ALL_ACCESS = 0x000f01ff;
        const uint LUA_TOKEN = 0x4, DISABLE_MAX_PRIVILEGE = 0x1;
        const int TokenSessionId = 12, TokenIntegrityLevel = 25;
        const uint CREATE_NEW_CONSOLE = 0x10, CREATE_UNICODE_ENVIRONMENT = 0x400, CREATE_NO_WINDOW = 0x08000000;

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
                    UserInteractive = Environment.UserInteractive, TokenSummary = DescribeToken(token),
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
        public static string CmdCommandLine(string executable, string command)
        {
            return QuoteArgument(executable) + " /d /s /c \"" + command + "\"";
        }
        public static void CheckLayout()
        {
            CheckAccessLayout();
            CheckJobLayout();
            // Fail before calling Win32 if Add-Type marshalling differs from the ABI.
            if (Marshal.SizeOf(typeof(StartupInfo)) != (IntPtr.Size == 8 ? 104 : 68) ||
                Marshal.SizeOf(typeof(SidAndAttributes)) != (IntPtr.Size == 8 ? 16 : 8) ||
                Marshal.SizeOf(typeof(TokenPointer)) != IntPtr.Size ||
                Marshal.SizeOf(typeof(ProcessInfo)) != (IntPtr.Size == 8 ? 24 : 16))
                throw new InvalidOperationException("unexpected Win32 structure layout");
        }
        public static RestrictedDesktop Open(int expectedSession, string workspace)
        {
            CheckLayout();
            return new RestrictedDesktop(expectedSession, workspace);
        }

        // Probes and the selected launch share one token per profile. Changing
        // profiles always derives a fresh token from the unchanged parent token.
        // High/unrepaired profiles are diagnostics only, never suite candidates.
        public sealed class RestrictedDesktop : IDisposable
        {
            IntPtr token, environment;
            DesktopAclScope desktopAcl;
            readonly string workspace;
            readonly int expectedSession;
            bool repairDefaults, lowerIntegrity, duplicatePrimary, disposed;
            public string TokenSummary { get; private set; }
            public string DesktopSecurity { get { return DescribeDesktopSecurity(); } }
            // The parent owns ACL restoration even if a supervised stage hangs.
            public void EnsureDesktopAccess()
            {
                if (desktopAcl == null) desktopAcl = DesktopAclScope.Grant(token);
            }
            public RestrictedDesktop(int expectedSession, string directory)
            {
                workspace = directory;
                this.expectedSession = expectedSession;
                Configure(true, true, false);
            }
            public void Configure(bool repairDefaults, bool lowerIntegrity, bool duplicatePrimary)
            {
                if (disposed) throw new ObjectDisposedException("RestrictedDesktop");
                if (token != IntPtr.Zero && this.repairDefaults == repairDefaults &&
                    this.lowerIntegrity == lowerIntegrity && this.duplicatePrimary == duplicatePrimary) return;
                ReleaseToken();
                this.repairDefaults = repairDefaults;
                this.lowerIntegrity = lowerIntegrity;
                this.duplicatePrimary = duplicatePrimary;
                IntPtr source = IntPtr.Zero, admins = IntPtr.Zero, medium = IntPtr.Zero;
                string before = "not read", after = "not read";
                try
                {
                    Check(OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, out source), "OpenProcessToken launch");
                    Check(ConvertStringSidToSidW("S-1-5-32-544", out admins), "Administrators SID");
                    // Disable only Administrators. In particular, preserve every
                    // enabled logon SID used by the interactive desktop's ACL.
                    SidAndAttributes[] disable = { new SidAndAttributes { Sid = admins } };
                    // Retained-High diagnostic isolates LUA filtering from the
                    // Medium label and privilege removal. It cannot run tests.
                    uint flags = LUA_TOKEN | (lowerIntegrity ? DISABLE_MAX_PRIVILEGE : 0);
                    Check(CreateRestrictedToken(source, flags, 1, disable,
                        0, IntPtr.Zero, 0, IntPtr.Zero, out token), "CreateRestrictedToken");
                    if (duplicatePrimary)
                    {
                        IntPtr primary;
                        Check(DuplicateTokenEx(token, TOKEN_ALL_ACCESS, IntPtr.Zero, 2, 1, out primary), "DuplicateTokenEx primary");
                        CloseHandle(token);
                        token = primary;
                    }
                    IntPtr type = TokenInfo(token, 8); // TokenType: TokenPrimary = 1
                    try
                    {
                        if (Marshal.ReadInt32(type) != 1) throw new InvalidOperationException("restricted launch token is not primary");
                    }
                    finally { Marshal.FreeHGlobal(type); }
                    before = DescribeTokenDefaults(token);
                    if (repairDefaults) RepairTokenDefaults(token);
                    after = DescribeTokenDefaults(token);
                    if (lowerIntegrity)
                    {
                        Check(ConvertStringSidToSidW("S-1-16-8192", out medium), "Medium integrity SID");
                        SidAndAttributes label = new SidAndAttributes { Sid = medium, Attributes = 0x20 };
                        Check(SetTokenInformation(token, TokenIntegrityLevel, ref label,
                            checked((uint)Marshal.SizeOf(typeof(SidAndAttributes)) + GetLengthSid(medium))), "SetTokenInformation Medium");
                    }
                    if (expectedSession <= 0 || Session(source) != expectedSession || Session(token) != expectedSession)
                        throw new InvalidOperationException("restricted token is not in the parent's interactive session");
                    VerifyLogonSids(source, token);
                    PreserveTraversal(token); // DISABLE_MAX_PRIVILEGE must retain SeChangeNotifyPrivilege.
                    TokenSummary = "RepairDefaults=" + repairDefaults + "; LowerIntegrity=" + lowerIntegrity +
                        "; DuplicatePrimary=" + duplicatePrimary + "; DefaultsBefore={" + before +
                        "}; DefaultsAfter={" + after + "}; " + DescribeToken(token);
                    environment = MakeEnvironment(token, workspace);
                }
                catch (Exception error)
                {
                    if (token != IntPtr.Zero)
                    {
                        string details;
                        try { details = DescribeToken(token); }
                        catch (Exception queryError) { details = "token diagnostics unavailable: " + queryError.Message; }
                        finally { ReleaseToken(); }
                        TokenSummary = "DefaultsBefore={" + before + "}; DefaultsAfter={" + after + "}; " + details;
                        throw new InvalidOperationException("restricted token setup failed: " + error.Message + "; " + TokenSummary, error);
                    }
                    throw;
                }
                finally
                {
                    if (medium != IntPtr.Zero) LocalFree(medium);
                    if (admins != IntPtr.Zero) LocalFree(admins);
                    if (source != IntPtr.Zero) CloseHandle(source);
                }
            }
            public Child Start(string executable, string[] arguments, string method, bool repairDesktop, bool newConsole, bool useShowWindow)
            {
                string command = QuoteArgument(executable);
                foreach (string argument in arguments) command += " " + QuoteArgument(argument);
                return StartCommand(executable, command, method, repairDesktop, newConsole, useShowWindow);
            }
            public Child StartCmd(string command, string method, bool repairDesktop, bool newConsole, bool useShowWindow)
            {
                string executable = Environment.GetEnvironmentVariable("SystemRoot") + @"\System32\cmd.exe";
                // cmd /S /C has its own quote rules, not the CRT argv rules.
                return StartCommand(executable, CmdCommandLine(executable, command), method, repairDesktop, newConsole, useShowWindow);
            }
            Child StartCommand(string executable, string command, string method, bool repairDesktop, bool newConsole, bool useShowWindow)
            {
                if (token == IntPtr.Zero) throw new ObjectDisposedException("RestrictedDesktop");
                if (repairDesktop) EnsureDesktopAccess();
                StartupInfo startup = new StartupInfo {
                    cb = (uint)Marshal.SizeOf(typeof(StartupInfo)), lpDesktop = @"WinSta0\Default",
                    dwFlags = useShowWindow ? 1u : 0u, wShowWindow = 0 // STARTF_USESHOWWINDOW / SW_HIDE contrast
                };
                ProcessInfo info;
                uint flags = (newConsole ? CREATE_NEW_CONSOLE : CREATE_NO_WINDOW) | CREATE_UNICODE_ENVIRONMENT | 4; // CREATE_SUSPENDED
                // No STARTF_USESTDHANDLES and no inherited handles: logs/done.json
                // are opened by the child inside its Medium workspace instead.
                bool created;
                if (method == "AsUser")
                    created = CreateProcessAsUserW(token, executable, new StringBuilder(command),
                        IntPtr.Zero, IntPtr.Zero, false, flags, environment, workspace, ref startup, out info);
                else if (method == "WithToken")
                    created = CreateProcessWithTokenW(token, 0, executable, new StringBuilder(command),
                        flags, environment, workspace, ref startup, out info);
                else throw new ArgumentException("unknown launch method: " + method);
                if (!created) throw Failure(method + " create", Marshal.GetLastWin32Error());
                return ContainAndResume(info);
            }
            public void Dispose()
            {
                try { if (desktopAcl != null) desktopAcl.Dispose(); }
                finally
                {
                    desktopAcl = null;
                    ReleaseToken();
                    disposed = true;
                }
            }
            void ReleaseToken()
            {
                if (environment != IntPtr.Zero) { Marshal.FreeHGlobal(environment); environment = IntPtr.Zero; }
                if (token != IntPtr.Zero) { CloseHandle(token); token = IntPtr.Zero; }
            }
        }

        public sealed class Child : IDisposable
        {
            readonly SafeWaitHandle handle;
            IntPtr job;
            internal void OwnJob(IntPtr owned) { job = owned; }
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
                // Closing the parent's sole job handle kills the whole stage,
                // including any child stuck in initialization, UIA or a dialog.
                if (job != IntPtr.Zero) { CloseHandle(job); job = IntPtr.Zero; }
                if (HasExited || TerminateProcess(handle, 1)) return;
                int error = Marshal.GetLastWin32Error();
                // Exit may race the zero-time wait; an already dead child needs no kill.
                if (!HasExited) throw Failure("terminate desktop child", error);
            }
            public void Dispose()
            {
                if (job != IntPtr.Zero) { CloseHandle(job); job = IntPtr.Zero; }
                handle.Dispose();
            }
        }
    }
}
