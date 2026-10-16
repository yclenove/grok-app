// CI-only default object security for restricted tokens; never used by the engine.
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;

namespace Gcu
{
    public static partial class DesktopLauncher
    {
        const int TokenOwner = 4, TokenDefaultDacl = 6;

        // TOKEN_OWNER and TOKEN_DEFAULT_DACL each contain exactly one pointer.
        [StructLayout(LayoutKind.Sequential)]
        struct TokenPointer { public IntPtr Value; }

        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool SetTokenInformation(IntPtr token, int kind, ref TokenPointer value, uint length);
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool GetSecurityDescriptorOwner(IntPtr descriptor, out IntPtr owner,
            [MarshalAs(UnmanagedType.Bool)] out bool defaulted);
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool SetSecurityDescriptorOwner(IntPtr descriptor, IntPtr owner,
            [MarshalAs(UnmanagedType.Bool)] bool defaulted);
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool DuplicateTokenEx(IntPtr existing, uint access, IntPtr attributes,
            int impersonationLevel, int tokenType, out IntPtr token);

        // Pure builder: only the actual user, enabled logon SIDs, and SYSTEM
        // receive access. A deny-only Administrators SID must never be the owner
        // or the sole trustee of objects created while user32 initializes.
        public static string DefaultTokenSecurity(string userSid, string[] logonSids)
        {
            if (userSid == null || !Regex.IsMatch(userSid, @"\AS-1-[0-9]+(?:-[0-9]+)+\z") || userSid == "S-1-5-32-544")
                throw new ArgumentException("invalid token user SID");
            SortedSet<string> logons = new SortedSet<string>(StringComparer.Ordinal);
            foreach (string sid in logonSids)
            {
                if (sid == null || !Regex.IsMatch(sid, @"\AS-1-5-5-[0-9]+-[0-9]+\z"))
                    throw new ArgumentException("invalid token logon SID");
                if (sid != userSid) logons.Add(sid);
            }
            StringBuilder text = new StringBuilder("O:" + userSid + "D:(A;;GA;;;" + userSid + ")");
            foreach (string sid in logons) text.Append("(A;;GA;;;" + sid + ")");
            return text.Append("(A;;GA;;;SY)").ToString();
        }

        static string DescriptorText(IntPtr descriptor)
        {
            IntPtr text;
            Check(ConvertSecurityDescriptorToStringSecurityDescriptorW(descriptor, 1, 1 | 4,
                out text, IntPtr.Zero), "token owner/default DACL SDDL");
            try { return Marshal.PtrToStringUni(text); }
            finally { LocalFree(text); }
        }

        static string DescribeTokenDefaults(IntPtr token)
        {
            IntPtr owner = IntPtr.Zero, dacl = IntPtr.Zero, descriptor = IntPtr.Zero;
            try
            {
                owner = TokenInfo(token, TokenOwner);
                dacl = TokenInfo(token, TokenDefaultDacl);
                descriptor = Marshal.AllocHGlobal(IntPtr.Size == 8 ? 40 : 20); // absolute SECURITY_DESCRIPTOR
                Check(InitializeSecurityDescriptor(descriptor, 1), "initialize token diagnostics descriptor");
                Check(SetSecurityDescriptorOwner(descriptor, Marshal.ReadIntPtr(owner), false), "attach token owner");
                Check(SetSecurityDescriptorDacl(descriptor, true, Marshal.ReadIntPtr(dacl), false), "attach token default DACL");
                return DescriptorText(descriptor);
            }
            finally
            {
                if (descriptor != IntPtr.Zero) Marshal.FreeHGlobal(descriptor);
                if (dacl != IntPtr.Zero) Marshal.FreeHGlobal(dacl);
                if (owner != IntPtr.Zero) Marshal.FreeHGlobal(owner);
            }
        }

        static void RepairTokenDefaults(IntPtr token)
        {
            List<string> logons = new List<string>();
            foreach (KeyValuePair<string, uint> group in Groups(token))
                if ((group.Value & 0xc0000000) == 0xc0000000 && (group.Value & 4) != 0 && (group.Value & 0x10) == 0)
                    logons.Add(group.Key);
            IntPtr descriptor;
            string sddl = DefaultTokenSecurity(UserSid(token), logons.ToArray());
            Check(ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl, 1, out descriptor, IntPtr.Zero),
                "restricted token user/logon/SYSTEM descriptor");
            try
            {
                IntPtr owner, dacl;
                bool present, defaulted;
                Check(GetSecurityDescriptorOwner(descriptor, out owner, out defaulted), "read new token owner");
                Check(GetSecurityDescriptorDacl(descriptor, out present, out dacl, out defaulted), "read new token default DACL");
                if (owner == IntPtr.Zero || !present || dacl == IntPtr.Zero)
                    throw new InvalidOperationException("restricted token security descriptor lacks owner or DACL");
                TokenPointer value = new TokenPointer { Value = owner };
                Check(SetTokenInformation(token, TokenOwner, ref value, (uint)IntPtr.Size), "SetTokenInformation TokenOwner=user");
                value.Value = dacl;
                Check(SetTokenInformation(token, TokenDefaultDacl, ref value, (uint)IntPtr.Size), "SetTokenInformation TokenDefaultDacl=user/logon/SYSTEM");
                // Compare native canonical SDDL, not the input aliases. This
                // readback is mandatory before any process uses the token.
                string actual = DescribeTokenDefaults(token), expected = DescriptorText(descriptor);
                if (!string.Equals(actual, expected, StringComparison.Ordinal))
                    throw new InvalidOperationException("token defaults readback mismatch: expected=" + expected + "; actual=" + actual);
            }
            finally { LocalFree(descriptor); }
        }
    }
}
