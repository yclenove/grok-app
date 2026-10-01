//! Opt-in native adapter acceptance. Not installed or bundled with Grok App.
pub use grok_computer_use_core::{adapter, protocol};
#[cfg(target_os = "macos")]
#[path = "../../../src/computer_use/macos_adapter.rs"]
pub mod macos_adapter;
mod macos_native_gates;
mod macos_native_pipe;
mod macos_native_pointer;
mod macos_native_pointer_gates;
mod macos_native_protocol;

#[cfg(target_os = "macos")]
fn native_process() -> Result<(), String> {
    let mut value: libc::c_int = 0;
    let mut size = std::mem::size_of_val(&value);
    let status = unsafe {
        libc::sysctlbyname(
            c"sysctl.proc_translated".as_ptr(),
            (&mut value as *mut libc::c_int).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    let missing =
        status == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT);
    macos_native_protocol::native_translation_readback(status, value, size, missing)
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3
        || args[0] != "--owned-cocoa"
        || std::env::var("GROK_CU_MACOS_FIXTURE").as_deref() != Ok("owned-cocoa")
    {
        eprintln!("not_run: opt in with GROK_CU_MACOS_FIXTURE=owned-cocoa cu-macos-native --owned-cocoa <fixture executable> <fresh evidence directory>");
        std::process::exit(2);
    }
    let output = std::path::Path::new(&args[2]);
    if let Err(error) = std::fs::create_dir(output) {
        eprintln!("not_run: evidence directory must be new: {error}");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    let report = match native_process() {
        Ok(()) => macos_native_gates::execute(
            Some(&macos_adapter::MacosAdapter::new()),
            std::path::Path::new(&args[1]),
            output,
        ),
        Err(error) => macos_native_gates::Report::not_run(&error),
    };
    #[cfg(not(target_os = "macos"))]
    let report = macos_native_gates::execute(None, std::path::Path::new(&args[1]), output);
    let bytes = serde_json::to_vec_pretty(&report).expect("report serialization");
    if let Err(error) = macos_native_gates::write_new(&output.join("result.json"), &bytes) {
        eprintln!("native evidence write failed: {error}");
        std::process::exit(1);
    }
    println!("{}", String::from_utf8_lossy(&bytes));
    std::process::exit(match report.status.as_str() {
        "passed" => 0,
        "not_run" => 2,
        _ => 1,
    });
}
