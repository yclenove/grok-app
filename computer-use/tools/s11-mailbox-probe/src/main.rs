use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match s11_mailbox_probe::execute(&args) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(msg) => {
            eprintln!("s11-mailbox-probe: {msg}");
            ExitCode::from(2)
        }
    }
}
