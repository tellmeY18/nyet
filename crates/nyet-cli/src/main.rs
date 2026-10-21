use std::process::ExitCode;

fn main() -> ExitCode {
    println!("nyet {} ({})", nyet_core::VERSION, nyet_core::engine_name());
    ExitCode::SUCCESS
}
