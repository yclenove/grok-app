//! Scripted stand-in for the Grok CLI. See [`gcu_eval::builtin_fake_agent_entry`].

fn main() -> std::process::ExitCode {
    gcu_eval::builtin_fake_agent_exit()
}
