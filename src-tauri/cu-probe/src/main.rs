//! Runs Computer Use Broker gates without the lib test harness.
//! Separate workspace package; never a Grok App bundle binary.
fn main() {
    let code = grok_app_lib::computer_use_probe_main();
    std::process::exit(code);
}
