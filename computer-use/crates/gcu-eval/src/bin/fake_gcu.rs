//! Stand-in for `gcu call` and `gcu mcp`. See [`gcu_eval::builtin_fake_gcu_entry`].

fn main() -> std::process::ExitCode {
    gcu_eval::builtin_fake_gcu_exit()
}
