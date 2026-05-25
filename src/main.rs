use std::env;
use std::process::ExitCode;

mod app;

fn main() -> ExitCode {
    match app::run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
