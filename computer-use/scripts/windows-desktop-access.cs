// CI-only token diagnostics, per-user Medium workspace, and reversible desktop ACL grants.
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;

namespace Gcu
{
    public static partial class DesktopLauncher
    {
        [StructLayout(LayoutKind.Sequential)]
        struct Luid { public uint Low; public int High; }
        [StructLayout(LayoutKind.Sequential)]
        struct LuidAndAttributes { public Luid Luid; public uint Attributes; }
        [StructLayout(LayoutKind.Sequential)]
        struct OnePrivilege { public uint Count; public LuidAndAttributes Entry; }
        [StructLayout(LayoutKind.Sequential)]
        struct OneGroup { public uint Count; public SidAndAttributes Entry; }
        [StructLayout(LayoutKind.Sequential)]
        struct Trustee
        {
            public IntPtr Multiple;
            public int Operation, Form, Type;
            public IntPtr Name;
        }
        [StructLayout(LayoutKind.Sequential)]
        struct ExplicitAccess { public uint Permissions, Mode, Inheritance; public Trustee Trustee; }

        static void CheckAccessLayout()
        {
            if (Marshal.SizeOf(typeof(LuidAndAttributes)) != 12 ||
                Marshal.OffsetOf(typeof(OnePrivilege), "Entry").ToInt32() != 4 ||
                Marshal.OffsetOf(typeof(OneGroup), "Entry").ToInt32() != (IntPtr.Size == 8 ? 8 : 4) ||
                Marshal.SizeOf(typeof(ExplicitAccess)) != (IntPtr.Size == 8 ? 48 : 32))
                throw new InvalidOperationException("unexpected token/ACL structure layout");
        }
        [DllImport("userenv.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool CreateEnvironmentBlock(out IntPtr environment, IntPtr token, [MarshalAs(UnmanagedType.Bool)] bool inherit);
        [DllImport("userenv.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool DestroyEnvironmentBlock(IntPtr environment);
        [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool LookupPrivilegeNameW(string system, ref Luid luid, StringBuilder name, ref uint length);
        [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool LookupPrivilegeValueW(string system, string name, out Luid luid);
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool AdjustTokenPrivileges(IntPtr token, [MarshalAs(UnmanagedType.Bool)] bool disableAll,
            ref OnePrivilege state, uint size, IntPtr previous, IntPtr needed);
        [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool ConvertStringSecurityDescriptorToSecurityDescriptorW(string text, uint revision, out IntPtr descriptor, IntPtr size);
        [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool SetFileSecurityW(string file, uint information, IntPtr descriptor);
        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool GetUserObjectSecurity(IntPtr handle, ref uint information,
            IntPtr descriptor, uint size, out uint needed);
        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool SetUserObjectSecurity(IntPtr handle, ref uint information, IntPtr descriptor);
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool GetSecurityDescriptorDacl(IntPtr descriptor,
            [MarshalAs(UnmanagedType.Bool)] out bool present, out IntPtr dacl, [MarshalAs(UnmanagedType.Bool)] out bool defaulted);
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool InitializeSecurityDescriptor(IntPtr descriptor, uint revision);
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool SetSecurityDescriptorDacl(IntPtr descriptor,
            [MarshalAs(UnmanagedType.Bool)] bool present, IntPtr dacl, [MarshalAs(UnmanagedType.Bool)] bool defaulted);
        [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
        static extern uint SetEntriesInAclW(uint count, [In] ExplicitAccess[] entries, IntPtr oldAcl, out IntPtr newAcl);
        [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        static extern bool ConvertSecurityDescriptorToStringSecurityDescriptorW(IntPtr descriptor, uint revision,
            uint information, out IntPtr text, IntPtr length);

        static string ObjectSecurity(IntPtr handle)
        {
            IntPtr descriptor = IntPtr.Zero, text = IntPtr.Zero;
            try
            {
                uint information = 4 | 0x10, needed; // DACL + mandatory integrity label (not a full SACL)
                GetUserObjectSecurity(handle, ref information, IntPtr.Zero, 0, out needed);
                if (needed == 0) throw Failure("desktop security size", Marshal.GetLastWin32Error());
                descriptor = Marshal.AllocHGlobal(checked((int)needed));
                Check(GetUserObjectSecurity(handle, ref information, descriptor, needed, out needed), "desktop security read");
                Check(ConvertSecurityDescriptorToStringSecurityDescriptorW(descriptor, 1, information, out text, IntPtr.Zero), "desktop security SDDL");
                return Marshal.PtrToStringUni(text);
            }
            catch (Exception error) { return "unavailable: " + error.Message; }
            finally
            {
                if (text != IntPtr.Zero) LocalFree(text);
                if (descriptor != IntPtr.Zero) Marshal.FreeHGlobal(descriptor);
            }
        }
        public static string DescribeDesktopSecurity()
        {
            return "WinSta0={" + ObjectSecurity(GetProcessWindowStation()) + "}; Default={" +
                ObjectSecurity(GetThreadDesktop(GetCurrentThreadId())) + "}";
        }

        static string UserSid(IntPtr token)
        {
            IntPtr info = TokenInfo(token, 1); // TokenUser
            try { return new SecurityIdentifier(Marshal.ReadIntPtr(info)).Value; }
            finally { Marshal.FreeHGlobal(info); }
        }
        static Dictionary<string, uint> Groups(IntPtr token)
        {
            IntPtr info = TokenInfo(token, 2); // TokenGroups
            try
            {
                Dictionary<string, uint> result = new Dictionary<string, uint>();
                int count = Marshal.ReadInt32(info);
                int offset = Marshal.OffsetOf(typeof(OneGroup), "Entry").ToInt32();
                for (int i = 0; i < count; i++)
                {
                    SidAndAttributes group = (SidAndAttributes)Marshal.PtrToStructure(
                        IntPtr.Add(info, checked(offset + i * Marshal.SizeOf(typeof(SidAndAttributes)))), typeof(SidAndAttributes));
                    result.Add(new SecurityIdentifier(group.Sid).Value, group.Attributes);
                }
                return result;
            }
            finally { Marshal.FreeHGlobal(info); }
        }
        static Dictionary<string, uint> Privileges(IntPtr token)
        {
            IntPtr info = TokenInfo(token, 3); // TokenPrivileges
            try
            {
                Dictionary<string, uint> result = new Dictionary<string, uint>();
                int count = Marshal.ReadInt32(info);
                int offset = Marshal.OffsetOf(typeof(OnePrivilege), "Entry").ToInt32();
                for (int i = 0; i < count; i++)
                {
                    LuidAndAttributes entry = (LuidAndAttributes)Marshal.PtrToStructure(
                        IntPtr.Add(info, checked(offset + i * Marshal.SizeOf(typeof(LuidAndAttributes)))), typeof(LuidAndAttributes));
                    uint length = 256;
                    StringBuilder name = new StringBuilder((int)length);
                    Check(LookupPrivilegeNameW(null, ref entry.Luid, name, ref length), "LookupPrivilegeName");
                    result.Add(name.ToString(), entry.Attributes);
                }
                return result;
            }
            finally { Marshal.FreeHGlobal(info); }
        }
        static void VerifyLogonSids(IntPtr source, IntPtr restricted)
        {
            CheckRestrictedGroups(Groups(source), Groups(restricted));
        }
        public static void CheckRestrictedGroups(IDictionary<string, uint> before, IDictionary<string, uint> after)
        {
            uint admin;
            if (after.TryGetValue("S-1-5-32-544", out admin) && (admin & 0x10) == 0)
                throw new InvalidOperationException("restricted Administrators SID is not deny-only");
            foreach (KeyValuePair<string, uint> group in before)
            {
                if ((group.Value & 0xc0000000) != 0xc0000000) continue; // SE_GROUP_LOGON_ID
                uint attributes;
                if (!after.TryGetValue(group.Key, out attributes) || (attributes & 0xc0000000) != 0xc0000000 ||
                    (attributes & 4) == 0 || (attributes & 0x10) != 0)
                    throw new InvalidOperationException("restricted token lost enabled logon SID " + group.Key);
            }
            // Some runner tokens lack a logon SID. Do not invent one: the ACL
            // variant can grant the user SID, and the child must still prove its desktop.
        }
        static void PreserveTraversal(IntPtr token)
        {
            uint attributes;
            if (!Privileges(token).TryGetValue("SeChangeNotifyPrivilege", out attributes))
                throw new InvalidOperationException("restricted token lost SeChangeNotifyPrivilege");
            if ((attributes & 2) == 0)
            {
                Luid luid;
                Check(LookupPrivilegeValueW(null, "SeChangeNotifyPrivilege", out luid), "lookup traversal privilege");
                OnePrivilege state = new OnePrivilege { Count = 1, Entry = new LuidAndAttributes { Luid = luid, Attributes = 2 } };
                Check(AdjustTokenPrivileges(token, false, ref state, 0, IntPtr.Zero, IntPtr.Zero), "enable traversal privilege");
            }
            if ((Privileges(token)["SeChangeNotifyPrivilege"] & 2) == 0)
                throw new InvalidOperationException("SeChangeNotifyPrivilege is not enabled");
        }
        static string DescribeToken(IntPtr token)
        {
            StringBuilder text = new StringBuilder("UserSid=" + UserSid(token) + "; SessionId=" + Session(token));
            IntPtr type = TokenInfo(token, 8); // TokenType
            try { text.Append("; TokenType=" + Marshal.ReadInt32(type)); }
            finally { Marshal.FreeHGlobal(type); }
            IntPtr info = TokenInfo(token, TokenIntegrityLevel);
            try { text.Append("; IntegritySid=" + new SecurityIdentifier(Marshal.ReadIntPtr(info)).Value); }
            finally { Marshal.FreeHGlobal(info); }
            Dictionary<string, uint> groups = Groups(token);
            text.Append("; LogonSids=[");
            foreach (KeyValuePair<string, uint> group in groups)
                if ((group.Value & 0xc0000000) == 0xc0000000) text.Append(group.Key + ",");
            text.Append("]; Groups=[");
            foreach (KeyValuePair<string, uint> group in groups)
                text.Append(group.Key + ":0x" + group.Value.ToString("X", CultureInfo.InvariantCulture) + ",");
            text.Append("]; Privileges=[");
            foreach (KeyValuePair<string, uint> privilege in Privileges(token))
                text.Append(privilege.Key + ":0x" + privilege.Value.ToString("X", CultureInfo.InvariantCulture) + ",");
            return text.Append("]; OwnerDefaultDacl={" + DescribeTokenDefaults(token) + "}").ToString();
        }
        static IntPtr MakeEnvironment(IntPtr token, string workspace)
        {
            IntPtr original;
            Check(CreateEnvironmentBlock(out original, token, false), "CreateEnvironmentBlock");
            try
            {
                SortedDictionary<string, string> values = new SortedDictionary<string, string>(StringComparer.OrdinalIgnoreCase);
                int offset = 0;
                while (Marshal.ReadInt16(original, offset) != 0)
                {
                    string entry = Marshal.PtrToStringUni(IntPtr.Add(original, offset));
                    int separator = entry.IndexOf('=', 1); // preserve any =C: drive entries
                    if (separator > 0) values[entry.Substring(0, separator)] = entry.Substring(separator + 1);
                    offset = checked(offset + (entry.Length + 1) * 2);
                }
                return Marshal.StringToHGlobalUni(EnvironmentText(values, workspace));
            }
            finally { Check(DestroyEnvironmentBlock(original), "DestroyEnvironmentBlock"); }
        }
        // Pure part of the Unicode environment builder, exercised on Linux too.
        public static string EnvironmentText(IDictionary<string, string> inherited, string workspace)
        {
            SortedDictionary<string, string> values = new SortedDictionary<string, string>(inherited, StringComparer.OrdinalIgnoreCase);
            values["TEMP"] = workspace;
            values["TMP"] = workspace;
            StringBuilder block = new StringBuilder();
            foreach (KeyValuePair<string, string> entry in values) block.Append(entry.Key + "=" + entry.Value).Append('\0');
            return block.Append('\0').ToString();
        }
        public static string PrepareWorkspace()
        {
            string root = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Temp");
            string path = Path.Combine(root, "wd" + Guid.NewGuid().ToString("N").Substring(0, 8));
            Directory.CreateDirectory(path);
            try
            {
                string user;
                using (WindowsIdentity identity = WindowsIdentity.GetCurrent()) user = identity.User.Value;
                IntPtr descriptor;
                // Per-user DACL and inheritable Medium label: both code and output
                // live here, independent of High-labeled runner/temp/build paths.
                string sddl = "D:P(A;OICI;FA;;;" + user + ")(A;OICI;FA;;;SY)S:(ML;OICI;NW;;;ME)";
                Check(ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl, 1, out descriptor, IntPtr.Zero), "workspace security descriptor");
                try { Check(SetFileSecurityW(path, 0x80000000 | 4 | 0x10, descriptor), "workspace protected user DACL + Medium label"); }
                finally { LocalFree(descriptor); }
                return path;
            }
            catch { Directory.Delete(path); throw; }
        }

        [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern IntPtr OpenWindowStationW(string name, [MarshalAs(UnmanagedType.Bool)] bool inherit, uint access);
        [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern IntPtr OpenDesktopW(string name, uint flags, [MarshalAs(UnmanagedType.Bool)] bool inherit, uint access);
        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool CloseWindowStation(IntPtr handle);
        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool CloseDesktop(IntPtr handle);

        sealed class DesktopAclScope : IDisposable
        {
            sealed class Saved { public IntPtr Handle, Descriptor; public string Name; public bool Station, Changed; }
            readonly System.Collections.Generic.List<Saved> saved = new System.Collections.Generic.List<Saved>();
            public static DesktopAclScope Grant(IntPtr token)
            {
                DesktopAclScope scope = new DesktopAclScope();
                try
                {
                    List<string> sids = new List<string> { UserSid(token) };
                    foreach (KeyValuePair<string, uint> group in Groups(token))
                        if ((group.Value & 0xc0000000) == 0xc0000000) sids.Add(group.Key);
                    scope.GrantObject("WinSta0", true, 0x000f037f, sids);
                    scope.GrantObject("Default", false, 0x000f01ff, sids);
                    return scope;
                }
                catch { scope.Dispose(); throw; }
            }
            void GrantObject(string name, bool station, uint rights, List<string> sids)
            {
                // Borrowed process/thread desktop handles may lack WRITE_DAC.
                // Open owned security handles without changing the parent's station.
                IntPtr handle = station ? OpenWindowStationW(name, false, 0x00060000) :
                    OpenDesktopW(name, 0, false, 0x00060081); // READ_CONTROL|WRITE_DAC|READ/WRITEOBJECTS
                if (handle == IntPtr.Zero) throw Failure("open desktop ACL handle " + name, Marshal.GetLastWin32Error());
                Saved entry = new Saved { Handle = handle, Name = name, Station = station };
                saved.Add(entry);
                uint information = 4, needed;
                GetUserObjectSecurity(handle, ref information, IntPtr.Zero, 0, out needed);
                if (needed == 0) throw Failure("desktop DACL size " + name, Marshal.GetLastWin32Error());
                IntPtr original = Marshal.AllocHGlobal(checked((int)needed));
                try { Check(GetUserObjectSecurity(handle, ref information, original, needed, out needed), "read desktop DACL " + name); }
                catch { Marshal.FreeHGlobal(original); throw; }
                entry.Descriptor = original;
                bool present, defaulted;
                IntPtr oldAcl;
                Check(GetSecurityDescriptorDacl(original, out present, out oldAcl, out defaulted), "extract desktop DACL " + name);
                if (!present || oldAcl == IntPtr.Zero) return; // null DACL already grants access; do not narrow it
                ExplicitAccess[] entries = new ExplicitAccess[sids.Count];
                IntPtr acl = IntPtr.Zero, descriptor = Marshal.AllocHGlobal(40); // SECURITY_DESCRIPTOR (x64)
                try
                {
                    for (int i = 0; i < sids.Count; i++)
                    {
                        IntPtr sid;
                        Check(ConvertStringSidToSidW(sids[i], out sid), "desktop trustee SID");
                        entries[i] = new ExplicitAccess { Permissions = rights, Mode = 1,
                            Trustee = new Trustee { Form = 0, Type = 0, Name = sid } };
                    }
                    uint error = SetEntriesInAclW((uint)entries.Length, entries, oldAcl, out acl);
                    if (error != 0) throw Failure("merge desktop DACL " + name, (int)error);
                    Check(InitializeSecurityDescriptor(descriptor, 1), "initialize desktop security descriptor");
                    Check(SetSecurityDescriptorDacl(descriptor, true, acl, false), "attach desktop DACL");
                    Check(SetUserObjectSecurity(handle, ref information, descriptor), "grant user/logon desktop access " + name);
                    entry.Changed = true;
                }
                finally
                {
                    foreach (ExplicitAccess access in entries) if (access.Trustee.Name != IntPtr.Zero) LocalFree(access.Trustee.Name);
                    if (acl != IntPtr.Zero) LocalFree(acl);
                    Marshal.FreeHGlobal(descriptor);
                }
            }
            public void Dispose()
            {
                List<string> errors = new List<string>();
                for (int i = saved.Count - 1; i >= 0; i--)
                {
                    uint information = 4;
                    Saved entry = saved[i];
                    if (entry.Changed && !SetUserObjectSecurity(entry.Handle, ref information, entry.Descriptor))
                        errors.Add("restore " + entry.Name + " DACL: Win32=" + Marshal.GetLastWin32Error());
                    if (entry.Descriptor != IntPtr.Zero) Marshal.FreeHGlobal(entry.Descriptor);
                    bool closed = entry.Station ? CloseWindowStation(entry.Handle) : CloseDesktop(entry.Handle);
                    if (!closed) errors.Add("close " + entry.Name + ": Win32=" + Marshal.GetLastWin32Error());
                }
                saved.Clear();
                if (errors.Count != 0) throw new InvalidOperationException(string.Join("; ", errors.ToArray()));
            }
        }
    }
}
