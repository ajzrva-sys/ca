//! Verification allowlist validation.
//!
//! Target home for program/argument validators and policy checks. Validation
//! should happen before any process execution.

use crate::verification::recipe::VerificationCommand;

/// Errors returned when a command fails allowlist validation.
#[derive(Debug, Clone, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum ValidationError {
    /// The command's program is not in the allowlist.
    ProgramNotAllowed { program: String },
    /// The command's args are not a valid prefix of the allowed recipe's args.
    ArgsMismatch {
        program: String,
        got_args: Vec<String>,
        allowed_args: Vec<String>,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProgramNotAllowed { program } => {
                write!(f, "program not in allowlist: {program}")
            }
            Self::ArgsMismatch {
                program,
                got_args,
                allowed_args,
            } => {
                write!(
                    f,
                    "args mismatch for program `{program}`: got [{}] not a prefix of allowed [{}]",
                    got_args.join(", "),
                    allowed_args.join(", "),
                )
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Validate that `command` matches at least one entry in `allowed_recipes`.
///
/// Validation rules:
/// 1. The command's `program` must exactly match one of the allowed recipe's
///    `program` fields.
/// 2. The command's `args` must be a **prefix** of the matching recipe's args
///    (i.e. every arg in `command.args` must equal the corresponding arg in the
///    recipe at the same index). This allows a caller to pass a subset of the
///    recipe's args without exceeding them.
#[allow(dead_code)]
pub(crate) fn validate_command(
    command: &VerificationCommand,
    allowed_recipes: &[VerificationCommand],
) -> Result<(), ValidationError> {
    // Find all recipes whose program matches.
    let matching: Vec<&VerificationCommand> = allowed_recipes
        .iter()
        .filter(|r| r.program == command.program)
        .collect();

    if matching.is_empty() {
        return Err(ValidationError::ProgramNotAllowed {
            program: command.program.clone(),
        });
    }

    // Check whether command.args is a prefix of any matching recipe's args.
    for recipe in &matching {
        if args_are_prefix(&command.args, &recipe.args) {
            return Ok(());
        }
    }

    // No matching recipe had a compatible args prefix. Report against the first
    // match's args as the representative allowed set.
    Err(ValidationError::ArgsMismatch {
        program: command.program.clone(),
        got_args: command.args.clone(),
        allowed_args: matching[0].args.clone(),
    })
}

/// Returns true when `got` is a prefix of `allowed` — every element of `got`
/// at index `i` equals `allowed[i]`, and `got.len() <= allowed.len()`.
#[allow(dead_code)]
fn args_are_prefix(got: &[String], allowed: &[String]) -> bool {
    if got.len() > allowed.len() {
        return false;
    }
    got.iter().zip(allowed.iter()).all(|(g, a)| g == a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(program: &str, args: &[&str]) -> VerificationCommand {
        VerificationCommand::new(
            program.to_string(),
            args.iter().map(|s| s.to_string()).collect(),
            "test",
        )
    }

    #[test]
    fn reject_unknown_program() {
        let allowed = vec![recipe("cargo", &["check"])];
        let cmd = recipe("rm", &["-rf", "/"]);
        let err = validate_command(&cmd, &allowed).unwrap_err();
        assert!(
            matches!(err, ValidationError::ProgramNotAllowed { ref program } if program == "rm"),
            "expected ProgramNotAllowed for unknown program, got: {err}"
        );
    }

    #[test]
    fn accept_allowed_program_with_matching_prefix_args() {
        let allowed = vec![recipe("cargo", &["test", "--all"])];
        // Exact match
        let cmd = recipe("cargo", &["test", "--all"]);
        assert!(validate_command(&cmd, &allowed).is_ok());

        // Prefix match (fewer args than recipe)
        let cmd_prefix = recipe("cargo", &["test"]);
        assert!(validate_command(&cmd_prefix, &allowed).is_ok());
    }

    #[test]
    fn reject_wrong_args() {
        let allowed = vec![recipe("cargo", &["test", "--all"])];
        // "cargo build" has the right program but wrong first arg
        let cmd = recipe("cargo", &["build"]);
        let err = validate_command(&cmd, &allowed).unwrap_err();
        assert!(
            matches!(err, ValidationError::ArgsMismatch { ref program, .. } if program == "cargo"),
            "expected ArgsMismatch for wrong args, got: {err}"
        );
    }

    #[test]
    fn reject_args_longer_than_recipe() {
        let allowed = vec![recipe("cargo", &["check"])];
        let cmd = recipe("cargo", &["check", "--extra"]);
        let err = validate_command(&cmd, &allowed).unwrap_err();
        assert!(
            matches!(err, ValidationError::ArgsMismatch { .. }),
            "expected ArgsMismatch for extra args, got: {err}"
        );
    }

    #[test]
    fn accept_command_matching_any_of_multiple_recipes() {
        let allowed = vec![
            recipe("cargo", &["check"]),
            recipe("cargo", &["test", "--all"]),
            recipe("rg", &["--version"]),
        ];
        // Should match "cargo test --all"
        let cmd = recipe("cargo", &["test"]);
        assert!(validate_command(&cmd, &allowed).is_ok());

        // Should match "rg --version"
        let cmd_rg = recipe("rg", &["--version"]);
        assert!(validate_command(&cmd_rg, &allowed).is_ok());
    }
}
