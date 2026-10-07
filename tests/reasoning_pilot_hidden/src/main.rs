use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../.."));
    match reasoning_pilot_hidden_verifier::fixture_check(&root) {
        Ok(receipt) => {
            for candidate in &receipt.candidates {
                println!(
                    "{} {} public={} hidden={} failure={}",
                    candidate.kind,
                    candidate.fixture_id,
                    candidate.public_passed,
                    candidate.hidden_passed,
                    candidate.hidden_failure_code.as_deref().unwrap_or("none")
                );
            }
            println!("fixture_check_passed={}", receipt.passed);
            if receipt.passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("fixture check failed: {error}");
            ExitCode::FAILURE
        }
    }
}
