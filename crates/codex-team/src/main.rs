use std::process::ExitCode;

fn main() -> ExitCode {
    match codex_team::cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let code = codex_team::release_readiness::requested_exit_code(&error).unwrap_or(1);
            eprintln!("Error: {error:?}");
            ExitCode::from(code)
        }
    }
}
