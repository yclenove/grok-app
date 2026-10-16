// CI-only elevated fixture: retain the parent's token, never activate its HWND.
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;

namespace Gcu
{
    public sealed class FixtureTokenFacts
    {
        public int Pid, SessionId, IntegrityRid, TokenElevation;
        public string IntegritySid;
        public override string ToString()
        {
            return "pid=" + Pid + "; GetTokenInformation(TokenElevation)=" + TokenElevation +
                "; GetTokenInformation(TokenIntegrityLevel)=" + IntegritySid + "; SessionId=" + SessionId;
        }
    }
    public static partial class DesktopLauncher
    {
        [DllImport("kernel32.dll", SetLastError = true)]
        static extern IntPtr OpenProcess(uint access, [MarshalAs(UnmanagedType.Bool)] bool inherit, int pid);

        public static FixtureTokenFacts ReadFixtureToken(int pid)
        {
            IntPtr process = OpenProcess(0x1000, false, pid), token = IntPtr.Zero;
            Check(process != IntPtr.Zero, "OpenProcess fixture token query");
            try
            {
                Check(OpenProcessToken(process, 8, out token), "OpenProcessToken fixture");
                FixtureTokenFacts facts = new FixtureTokenFacts { Pid = pid, SessionId = Session(token) };
                IntPtr info = TokenInfo(token, 20); // TokenElevation
                try { facts.TokenElevation = Marshal.ReadInt32(info); }
                finally { Marshal.FreeHGlobal(info); }
                info = TokenInfo(token, TokenIntegrityLevel);
                try { facts.IntegritySid = new SecurityIdentifier(Marshal.ReadIntPtr(info)).Value; }
                finally { Marshal.FreeHGlobal(info); }
                facts.IntegrityRid = int.Parse(facts.IntegritySid.Substring(facts.IntegritySid.LastIndexOf('-') + 1));
                return facts;
            }
            finally { if (token != IntPtr.Zero) CloseHandle(token); CloseHandle(process); }
        }

        public static Process StartUnactivatedFixture(string executable)
        {
            // CreateProcessW inherits this verified High parent primary token.
            // ShellExecute/Start-Process may delegate to a Medium shell instead.
            StartupInfo startup = new StartupInfo {
                cb = (uint)Marshal.SizeOf(typeof(StartupInfo)), lpDesktop = @"WinSta0\Default",
                dwFlags = 1, wShowWindow = 4 // STARTF_USESHOWWINDOW / SW_SHOWNOACTIVATE
            };
            ProcessInfo info;
            Check(CreateProcessW(executable, new StringBuilder(QuoteArgument(executable)),
                IntPtr.Zero, IntPtr.Zero, false, CREATE_UNICODE_ENVIRONMENT, IntPtr.Zero,
                System.IO.Path.GetDirectoryName(executable), ref startup, out info), "CreateProcessW High fixture");
            try { return Process.GetProcessById(checked((int)info.dwProcessId)); }
            finally { CloseHandle(info.hThread); CloseHandle(info.hProcess); }
        }
    }
}
