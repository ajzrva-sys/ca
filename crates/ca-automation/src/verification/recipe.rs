//! Rust-owned verification recipes and command contracts.
//!
//! AI may suggest intent; Rust validates and expands recipes into typed command
//! data before anything reaches process execution.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerificationCommand {
    // Commands are data, not shell. Rust owns sequencing, cwd, timeouts, output
    // capture, and failure policy. Legacy strings are parsed into this shape as
    // a migration bridge, then validated before execution.
    #[serde(default)]
    pub(crate) id: String,
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    #[serde(default, alias = "timeout_secs")]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) kind: CommandKind,
    #[serde(default)]
    pub(crate) failure_policy: FailurePolicy,
    #[serde(default)]
    pub(crate) expected_exit: Option<i32>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) source: String,
}

impl VerificationCommand {
    pub(crate) fn new(program: String, args: Vec<String>, source: &str) -> Self {
        Self {
            program,
            args,
            id: String::new(),
            cwd: None,
            timeout_seconds: None,
            kind: CommandKind::Unknown,
            failure_policy: FailurePolicy::Stop,
            expected_exit: None,
            tags: Vec::new(),
            source: source.to_string(),
        }
    }

    pub(crate) fn with_source(mut self, source: &str) -> Self {
        self.source = source.to_string();
        self
    }

    pub(crate) fn display(&self) -> String {
        // Display is for humans/logs only. Execution uses `program` and `args`
        // directly via std::process::Command.
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(display_command_token)
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub(crate) fn fingerprint(&self) -> String {
        // Full identity includes cwd and expected exit so run/evidence IDs do
        // not collide across execution contexts.
        let mut key = self.program.clone();
        key.push('\0');
        key.push_str(&self.args.join("\0"));
        key.push('\0');
        key.push_str(self.cwd.as_deref().unwrap_or(""));
        key.push('\0');
        key.push_str(
            &self
                .expected_exit
                .map(|code| code.to_string())
                .unwrap_or_default(),
        );
        key
    }

    pub(crate) fn semantic_fingerprint(&self) -> String {
        // Dedup handoff commands against task commands by "what it runs" rather
        // than by source/id. This avoids double-running the same final check.
        let mut key = self.program.clone();
        key.push('\0');
        key.push_str(&self.args.join("\0"));
        key.push('\0');
        key.push_str(&self.expected_exit.unwrap_or(0).to_string());
        key
    }

    pub(crate) fn with_id_and_metadata(mut self, id: String, kind: CommandKind) -> Self {
        self.id = id;
        if self.kind == CommandKind::Unknown {
            self.kind = kind;
        }
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub(crate) enum CommandKind {
    Build,
    Test,
    Lint,
    Search,
    Git,
    Smoke,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub(crate) enum FailurePolicy {
    // Stop keeps the final verification chain CI-like; Collect lets future
    // diagnostics gather more evidence after a known non-fatal failure.
    #[default]
    Stop,
    Collect,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum Recipe {
    // Recipes are Rust-owned command intents. Legacy shell-ish strings are
    // accepted only by parsing into one of these variants and validating fields.
    CargoCheck,
    CargoMetadata,
    CargoRustcMetadata {
        package: String,
    },
    CargoRustdocPublic {
        package: String,
    },
    CargoCheckPackage {
        package: String,
    },
    CargoTestPackage {
        package: String,
        filter: Option<String>,
    },
    NpmTest,
    NpmRunTest,
    NpmRunBuild,
    NpmWorkspaceTest {
        package: String,
    },
    PnpmTest,
    PnpmBuild,
    PnpmFilterTest {
        package: String,
    },
    YarnTest,
    YarnBuild,
    YarnWorkspaceTest {
        package: String,
    },
    Pytest {
        args: Vec<String>,
    },
    PythonPytest {
        args: Vec<String>,
    },
    SwiftDumpPackage,
    CargoBuild,
    CargoTestAll,
    CargoTestNamed {
        filter: String,
    },
    CargoClippy,
    CargoFmtCheck,
    SwiftBuild {
        product: Option<String>,
    },
    SwiftReleaseBuild {
        product: String,
    },
    SwiftStackerBenchmarkRun {
        suite: String,
        report_root: String,
        generate_gallery: bool,
    },
    SwiftTest {
        filter: Option<String>,
    },
    SwiftVersion,
    GitDiffCheck,
    GitStatusShort,
    GitDiffStat,
    Ripgrep {
        pattern: String,
        paths: Vec<String>,
        max_matches: usize,
    },
    LocalSmoke {
        binary: String,
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub(crate) enum CachePolicy {
    Never,
    #[default]
    SameGitHead,
    RelevantFiles,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub(crate) struct RecipeSpec {
    // RecipeSpec carries policy next to the intent. Execution expands it into a
    // VerificationCommand after validation, which keeps "what to verify"
    // separate from "how Rust executes it."
    pub(crate) id: String,
    pub(crate) recipe: Recipe,
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    #[serde(default, alias = "timeout_secs")]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) kind: CommandKind,
    #[serde(default)]
    pub(crate) failure_policy: FailurePolicy,
    #[serde(default)]
    pub(crate) cache_policy: CachePolicy,
    #[serde(default)]
    pub(crate) expected_exit: Option<i32>,
    #[serde(default)]
    pub(crate) source: String,
}

fn display_command_token(token: &str) -> String {
    if token
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '='))
    {
        token.to_string()
    } else {
        format!("'{}'", token.replace('\'', "'\\''"))
    }
}
