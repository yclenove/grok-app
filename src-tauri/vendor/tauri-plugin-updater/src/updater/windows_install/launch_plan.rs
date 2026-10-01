//! Checked native launch data, compiled before the App cleanup barrier. This
//! owns no App handle/callback and is not deserializable from recovery storage.
use super::*;
use crate::config::WindowsUpdateInstallMode;
use std::os::windows::ffi::{OsStrExt, OsStringExt};

pub(super) struct LaunchPlan {
    file: PathBuf,
    arguments: OsString,
}

impl LaunchPlan {
    pub fn new(
        installer: &WindowsUpdaterType,
        mode: WindowsUpdateInstallMode,
        current_args: &[OsString],
        installer_args: &[OsString],
    ) -> std::result::Result<Self, String> {
        let path = match installer {
            WindowsUpdaterType::Nsis { path, .. } | WindowsUpdaterType::Msi { path, .. } => path,
        };
        if !path.is_absolute() {
            return Err("Installer path must be absolute".into());
        }
        check_nul(path.as_os_str())?;
        // Validate even argv[0], which is deliberately not forwarded.
        for argument in current_args.iter().chain(installer_args) {
            check_nul(argument)?;
        }
        if installer_args.iter().any(|arg| {
            arg.to_string_lossy()
                .to_ascii_lowercase()
                .contains("/grokupdate")
        }) {
            return Err(
                "Reserved completion arguments cannot come from installer overrides".into(),
            );
        }
        let current_args = current_args.get(1..).unwrap_or_default();
        let (file, args): (PathBuf, Vec<OsString>) = match installer {
            WindowsUpdaterType::Nsis { .. } => {
                let mut args: Vec<OsString> = mode.nsis_args().iter().map(OsString::from).collect();
                args.extend([OsString::from("/UPDATE"), OsString::from("/ARGS")]);
                args.extend(current_args.iter().map(escape_nsis));
                args.extend_from_slice(installer_args);
                (path.clone(), args)
            }
            WindowsUpdaterType::Msi { .. } => {
                let mut args = vec![OsString::from("/i"), path.wrap_in_quotes().into_os_string()];
                args.extend(mode.msiexec_args().iter().map(OsString::from));
                args.push(OsString::from("/promptrestart"));
                args.extend_from_slice(installer_args);
                args.push(OsString::from("AUTOLAUNCHAPP=True"));
                let mut restart = OsString::from("LAUNCHAPPARGS=\"");
                restart.push(
                    current_args
                        .iter()
                        .map(escape_msi)
                        .collect::<Vec<_>>()
                        .join(OsStr::new(" ")),
                );
                restart.push("\"");
                args.push(restart);
                // Never resolve msiexec through PATH, CWD, or environment data.
                (
                    PathBuf::from(windows_system_directory()?).join("msiexec.exe"),
                    args,
                )
            }
        };
        let arguments = args.join(OsStr::new(" "));
        validate_command(&file, &arguments)?;
        Ok(Self { file, arguments })
    }

    pub fn launch(&self) -> std::result::Result<Option<OwnedHandle>, String> {
        launch_process(&self.file, &self.arguments)
    }

    pub(super) fn with_completion_receipt(
        mut self,
        id: &str,
        receipt: &Path,
    ) -> std::result::Result<Self, String> {
        if !receipt.is_absolute() {
            return Err("Completion receipt path must be absolute".into());
        }
        check_nul(receipt.as_os_str())?;
        let mut arguments = OsString::from(format!("/GROKUPDATEID={id} /GROKUPDATERECEIPT="));
        arguments.push(receipt.to_path_buf().wrap_in_quotes().into_os_string());
        arguments.push(" ");
        arguments.push(&self.arguments);
        validate_command(&self.file, &arguments)?;
        self.arguments = arguments;
        Ok(self)
    }

    pub(super) fn with_failure_receipt(
        mut self,
        receipt: &Path,
    ) -> std::result::Result<Self, String> {
        if !receipt.is_absolute() {
            return Err("Failure receipt path must be absolute".into());
        }
        check_nul(receipt.as_os_str())?;
        let mut arguments = OsString::from("/GROKUPDATEFAILURE=");
        arguments.push(receipt.to_path_buf().wrap_in_quotes().into_os_string());
        arguments.push(" ");
        arguments.push(&self.arguments);
        validate_command(&self.file, &arguments)?;
        self.arguments = arguments;
        Ok(self)
    }
}

fn check_nul(value: &OsStr) -> std::result::Result<(), String> {
    if value.encode_wide().any(|c| c == 0) {
        return Err("Windows launch data contains an embedded NUL".into());
    }
    Ok(())
}

pub(super) fn validate_command(file: &Path, arguments: &OsStr) -> std::result::Result<(), String> {
    if !file.is_absolute() {
        return Err("Windows launch executable must be absolute".into());
    }
    check_nul(file.as_os_str())?;
    check_nul(arguments)?;
    // Quotes around argv[0], one separating space, and the terminating NUL.
    if file.as_os_str().encode_wide().count() + arguments.encode_wide().count() + 4 > 32767 {
        return Err("Windows launch command exceeds the UTF-16 command-line limit".into());
    }
    Ok(())
}

// Preserve the pinned upstream NSIS/MSI escaping, but work on native code units
// rather than to_string_lossy(): unpaired Windows surrogates must not mutate an
// existing argument into a different path on restart.
fn escape_nsis(argument: &OsString) -> OsString {
    let arg: Vec<u16> = argument.encode_wide().collect();
    let quote = arg.is_empty() || arg.iter().any(|c| [32, 9, 47].contains(c));
    let mut result = Vec::new();
    if quote {
        result.push(34);
    }
    let mut slashes = 0;
    for c in arg {
        if c == 92 {
            slashes += 1;
        } else {
            if c == 34 {
                result.resize(result.len() + slashes + 1, 92);
            }
            slashes = 0;
        }
        result.push(c);
    }
    if quote {
        result.resize(result.len() + slashes, 92);
        result.push(34);
    }
    OsString::from_wide(&result)
}

fn escape_msi(argument: &OsString) -> OsString {
    let arg: Vec<u16> = argument.encode_wide().collect();
    if arg.is_empty() {
        return OsString::from("\"\"\"\"");
    }
    if !arg.contains(&32) && !arg.contains(&34) {
        return argument.clone();
    }
    let mut escaped = Vec::new();
    for c in arg {
        if c == 34 {
            escaped.extend([34, 34, 34, 34]);
        } else {
            escaped.push(c);
        }
    }
    let split = if escaped.first() == Some(&45) {
        escaped.iter().position(|c| *c == 61).map(|i| i + 1)
    } else {
        None
    };
    let split = split.unwrap_or(0);
    let mut result = escaped[..split].to_vec();
    result.extend([34, 34]);
    result.extend(&escaped[split..]);
    result.extend([34, 34]);
    OsString::from_wide(&result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_escaping_matches_pinned_upstream_for_valid_unicode() {
        // Bounded exhaustive grammar plus non-ASCII/supplementary characters.
        let alphabet = ['a', ' ', '\t', '/', '\\', '"', '-', '=', '中', '🦀'];
        let mut values = vec![String::new()];
        for _ in 0..4 {
            let previous = values.clone();
            values.extend(
                previous
                    .iter()
                    .flat_map(|v| alphabet.map(|c| format!("{v}{c}"))),
            );
        }
        values.sort();
        values.dedup();
        assert_eq!(values.len(), 11111);
        for value in values {
            let arg = OsString::from(&value);
            assert_eq!(
                escape_nsis(&arg),
                OsString::from(escape_nsis_current_exe_arg(&arg.as_os_str())),
                "NSIS {value:?}"
            );
            assert_eq!(
                escape_msi(&arg),
                OsString::from(escape_msi_property_arg(&arg)),
                "MSI {value:?}"
            );
        }
    }

    #[test]
    fn native_escaping_preserves_unpaired_utf16_without_replacement() {
        let arg = OsString::from_wide(&[45, 120, 61, 0xd800, 32, 0xdc00, 92]);
        assert_eq!(
            escape_nsis(&arg).encode_wide().collect::<Vec<_>>(),
            [34, 45, 120, 61, 0xd800, 32, 0xdc00, 92, 92, 34]
        );
        assert_eq!(
            escape_msi(&arg).encode_wide().collect::<Vec<_>>(),
            [45, 120, 61, 34, 34, 0xd800, 32, 0xdc00, 92, 34, 34]
        );
    }

    #[test]
    fn plan_preserves_all_modes_argument_order_and_system_msi_path() {
        for mode in [
            WindowsUpdateInstallMode::BasicUi,
            WindowsUpdateInstallMode::Quiet,
            WindowsUpdateInstallMode::Passive,
        ] {
            let args = vec![
                OsString::from("ignored-app.exe"),
                OsString::from("中 文"),
                OsString::from("/S"),
                OsString::from(""),
            ];
            let custom = vec![OsString::from("CUSTOM=Value")];
            let nsis_path = PathBuf::from(r"C:\owned space\signed.exe");
            let nsis = LaunchPlan::new(
                &WindowsUpdaterType::nsis(nsis_path.clone(), None),
                mode.clone(),
                &args,
                &custom,
            )
            .unwrap();
            assert_eq!(nsis.file, nsis_path);
            let prefix = mode.nsis_args().join(" ");
            assert_eq!(
                nsis.arguments.to_str().unwrap(),
                format!("{prefix} /UPDATE /ARGS \"中 文\" \"/S\" \"\" CUSTOM=Value").trim_start()
            );
            let msi = LaunchPlan::new(
                &WindowsUpdaterType::msi(PathBuf::from(r"C:\owned space\signed.msi"), None),
                mode.clone(),
                &args,
                &custom,
            )
            .unwrap();
            assert_eq!(
                msi.file,
                PathBuf::from(windows_system_directory().unwrap()).join("msiexec.exe")
            );
            assert_eq!(msi.arguments.to_str().unwrap(), format!("/i \"C:\\owned space\\signed.msi\" {} /promptrestart CUSTOM=Value AUTOLAUNCHAPP=True LAUNCHAPPARGS=\"\"\"中 文\"\" /S \"\"\"\"\"", mode.msiexec_args().join(" ")));
        }
    }

    #[test]
    fn completion_plan_prefix_preserves_unicode_and_cannot_be_overridden() {
        let installer = WindowsUpdaterType::nsis(PathBuf::from(r"C:\owned.exe"), None);
        let id = "01234567-89ab-4cde-8fab-0123456789ab";
        let receipt = Path::new(r"C:\中文 journal\receipt.nsis-complete");
        let original = LaunchPlan::new(
            &installer,
            Default::default(),
            &[
                OsString::from("app"),
                OsString::from("/GROKUPDATEID=forwarded-app-argument"),
            ],
            &[],
        )
        .unwrap();
        let suffix = original.arguments.clone();
        let bound = original.with_completion_receipt(id, receipt).unwrap();
        let mut expected = OsString::from(format!("/GROKUPDATEID={id} /GROKUPDATERECEIPT=\""));
        expected.push(receipt);
        expected.push("\" ");
        expected.push(suffix);
        assert_eq!(bound.arguments, expected);
        for argument in [
            "/GROKUPDATEID=evil",
            "/grokupdatereceipt=evil",
            "foo /GrOkUpDaTeId=evil",
            "/GROKUPDATEFAILURE=evil",
        ] {
            assert!(
                LaunchPlan::new(&installer, Default::default(), &[], &[argument.into()]).is_err()
            );
        }
        for receipt in [
            PathBuf::from("relative"),
            PathBuf::from("C:\\bad\0path"),
            PathBuf::from(format!("C:\\{}", "🦀".repeat(16384))),
        ] {
            assert!(LaunchPlan::new(&installer, Default::default(), &[], &[])
                .unwrap()
                .with_completion_receipt(id, &receipt)
                .is_err());
        }
        let native = PathBuf::from(OsString::from_wide(&[67, 58, 92, 0xd800, 32, 0xdc00]));
        let bound = LaunchPlan::new(&installer, Default::default(), &[], &[])
            .unwrap()
            .with_completion_receipt(id, &native)
            .unwrap();
        let units: Vec<_> = bound.arguments.encode_wide().collect();
        assert!(units
            .windows(6)
            .any(|part| part == [67, 58, 92, 0xd800, 32, 0xdc00]));
    }

    #[test]
    fn failure_plan_prefix_is_native_quoted_bounded_and_preserves_success_prefix() {
        let installer = WindowsUpdaterType::nsis(PathBuf::from(r"C:\owned.exe"), None);
        let plan = || LaunchPlan::new(&installer, Default::default(), &[], &[]).unwrap();
        let completed = plan()
            .with_completion_receipt(
                "01234567-89ab-4cde-8fab-0123456789ab",
                Path::new(r"C:\owned\ok"),
            )
            .unwrap();
        let suffix = completed.arguments.clone();
        let path = Path::new(r"C:\中文 空格 $ 🙂\failure.nsis-failed");
        let bound = completed.with_failure_receipt(path).unwrap();
        let mut expected = OsString::from("/GROKUPDATEFAILURE=\"");
        expected.push(path);
        expected.push("\" ");
        expected.push(suffix);
        assert_eq!(bound.arguments, expected);
        for invalid in [
            PathBuf::from("relative"),
            PathBuf::from("C:\\bad\0path"),
            PathBuf::from(format!("C:\\{}", "🙂".repeat(16384))),
        ] {
            assert!(plan().with_failure_receipt(&invalid).is_err());
        }
    }

    #[test]
    fn launch_data_rejects_nul_relative_paths_and_overlong_native_commands() {
        let installer = WindowsUpdaterType::nsis(PathBuf::from(r"C:\owned.exe"), None);
        for args in [
            vec![OsString::from("bad\0argv0")],
            vec![OsString::from("app"), OsString::from("ok\0hidden")],
        ] {
            assert!(LaunchPlan::new(&installer, Default::default(), &args, &[]).is_err());
        }
        assert!(LaunchPlan::new(
            &installer,
            Default::default(),
            &[],
            &[OsString::from("/P\0/S")]
        )
        .is_err());
        assert!(LaunchPlan::new(
            &WindowsUpdaterType::nsis(PathBuf::from("relative.exe"), None),
            Default::default(),
            &[],
            &[]
        )
        .is_err());
        assert!(validate_command(Path::new("C:\\safe.exe\0evil"), OsStr::new("")).is_err());
        assert!(validate_command(Path::new(r"C:\safe.exe"), OsStr::new("good\0evil")).is_err());
        assert!(LaunchPlan::new(
            &installer,
            Default::default(),
            &[],
            &[OsString::from("🦀".repeat(16384))]
        )
        .is_err());
        let path = Path::new(r"C:\safe.exe");
        let budget = 32767 - path.as_os_str().encode_wide().count() - 4;
        assert!(validate_command(path, OsStr::new(&"x".repeat(budget))).is_ok());
        assert!(validate_command(path, OsStr::new(&"x".repeat(budget + 1))).is_err());
    }
}
