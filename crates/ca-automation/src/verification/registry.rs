//! Canonical typed verification recipe registry.
//!
//! Every caller—legacy task parsing, learned recipes, MCP delegation, and the
//! pre-Worker router—must enter through this module before process execution.
//! It converts a deliberately small shell-free vocabulary into
//! [`VerificationCommand`] values and applies the final structured allowlist.

use crate::verification::recipe::{
    CachePolicy, CommandKind, FailurePolicy, Recipe, RecipeSpec, VerificationCommand,
};

pub(crate) fn parse_verification_command(
    command: &str,
    source: &str,
) -> Option<VerificationCommand> {
    crate::verification::ca::parse_ca_verification_command(command, source)
        .or_else(|| parse_legacy_recipe_spec(command, source).map(recipe_spec_to_command))
}

pub(crate) fn parse_legacy_recipe_spec(command: &str, source: &str) -> Option<RecipeSpec> {
    recipe_spec_from_words(split_command_words_strict(command)?, source)
}

pub(crate) fn recipe_spec_from_words(parts: Vec<String>, source: &str) -> Option<RecipeSpec> {
    let program = parts.first()?.as_str();
    let args = parts.iter().skip(1).map(String::as_str).collect::<Vec<_>>();
    let recipe = match program {
        "cargo" => match args.as_slice() {
            ["check"] => Recipe::CargoCheck,
            ["build"] => Recipe::CargoBuild,
            ["test"] => Recipe::CargoTestAll,
            ["test", filter] if safe_filter_token(filter) => Recipe::CargoTestNamed {
                filter: (*filter).to_string(),
            },
            ["clippy"] => Recipe::CargoClippy,
            ["fmt", "--", "--check"] | ["fmt", "--check"] => Recipe::CargoFmtCheck,
            _ => return None,
        },
        "npm" => match args.as_slice() {
            ["test"] => Recipe::NpmTest,
            ["run", "test"] => Recipe::NpmRunTest,
            ["run", "build"] => Recipe::NpmRunBuild,
            _ => return None,
        },
        "pnpm" => match args.as_slice() {
            ["test"] => Recipe::PnpmTest,
            ["build"] => Recipe::PnpmBuild,
            _ => return None,
        },
        "yarn" => match args.as_slice() {
            ["test"] => Recipe::YarnTest,
            ["build"] => Recipe::YarnBuild,
            _ => return None,
        },
        "pytest" if safe_pytest_args(&args) => Recipe::Pytest {
            args: args.into_iter().map(str::to_string).collect(),
        },
        "python" if args.starts_with(&["-m", "pytest"]) && safe_pytest_args(&args[2..]) => {
            Recipe::PythonPytest {
                args: args.into_iter().skip(2).map(str::to_string).collect(),
            }
        }
        "swift" => match args.as_slice() {
            ["build"] => Recipe::SwiftBuild { product: None },
            ["build", "--product", product] if safe_filter_token(product) => Recipe::SwiftBuild {
                product: Some((*product).to_string()),
            },
            ["build", "-c", "release", "--product", product]
                if swift_release_product_allowed(product) =>
            {
                Recipe::SwiftReleaseBuild {
                    product: (*product).to_string(),
                }
            }
            ["run", "-c", "release", "SwiftStackerBench", "--suite", suite, "--report-root", report_root]
                if safe_json_path_token(suite) && safe_report_root_token(report_root) =>
            {
                Recipe::SwiftStackerBenchmarkRun {
                    suite: (*suite).to_string(),
                    report_root: (*report_root).to_string(),
                    generate_gallery: false,
                }
            }
            ["run", "-c", "release", "SwiftStackerBench", "--suite", suite, "--report-root", report_root, "--generate-gallery"]
                if safe_json_path_token(suite) && safe_report_root_token(report_root) =>
            {
                Recipe::SwiftStackerBenchmarkRun {
                    suite: (*suite).to_string(),
                    report_root: (*report_root).to_string(),
                    generate_gallery: true,
                }
            }
            ["test"] => Recipe::SwiftTest { filter: None },
            ["test", "--filter", filter] if safe_filter_token(filter) => Recipe::SwiftTest {
                filter: Some((*filter).to_string()),
            },
            ["--version"] => Recipe::SwiftVersion,
            _ => return None,
        },
        "git" => match args.as_slice() {
            ["diff", "--check"] => Recipe::GitDiffCheck,
            ["status", "--short"] => Recipe::GitStatusShort,
            ["diff", "--stat"] => Recipe::GitDiffStat,
            _ => return None,
        },
        "rg" => {
            if args.is_empty()
                || args
                    .iter()
                    .any(|arg| arg.starts_with('-') && *arg != "-n" && *arg != "--line-number")
            {
                return None;
            }
            let args = args
                .into_iter()
                .filter(|arg| *arg != "-n" && *arg != "--line-number")
                .collect::<Vec<_>>();
            let pattern = args.first()?.to_string();
            let paths = args
                .iter()
                .skip(1)
                .map(|path| (*path).to_string())
                .collect();
            Recipe::Ripgrep {
                pattern,
                paths,
                max_matches: 200,
            }
        }
        _ if program.starts_with(".build/debug/") => {
            if args.is_empty() || !args.iter().all(|arg| arg.starts_with("--local-")) {
                return None;
            }
            Recipe::LocalSmoke {
                binary: program.strip_prefix(".build/debug/")?.to_string(),
                args: args.into_iter().map(str::to_string).collect(),
            }
        }
        _ => return None,
    };
    let mut spec = RecipeSpec {
        id: String::new(),
        kind: recipe_kind(&recipe),
        recipe,
        cwd: None,
        timeout_seconds: None,
        failure_policy: FailurePolicy::Stop,
        cache_policy: CachePolicy::SameGitHead,
        expected_exit: None,
        source: source.to_string(),
    };
    spec.id = recipe_id(&spec);
    validate_recipe_spec(&spec).then_some(spec)
}

pub(crate) fn validate_recipe_spec(spec: &RecipeSpec) -> bool {
    if !verification_command_cwd_allowed(spec.cwd.as_deref()) {
        return false;
    }
    match &spec.recipe {
        Recipe::CargoTestNamed { filter } => safe_filter_token(filter),
        Recipe::CargoCheckPackage { package }
        | Recipe::CargoRustcMetadata { package }
        | Recipe::CargoRustdocPublic { package }
        | Recipe::NpmWorkspaceTest { package }
        | Recipe::PnpmFilterTest { package }
        | Recipe::YarnWorkspaceTest { package } => safe_package_token(package),
        Recipe::CargoTestPackage { package, filter } => {
            safe_package_token(package) && filter.as_deref().map(safe_filter_token).unwrap_or(true)
        }
        Recipe::Pytest { args } | Recipe::PythonPytest { args } => {
            safe_pytest_args(&args.iter().map(String::as_str).collect::<Vec<_>>())
        }
        Recipe::SwiftBuild { product } => product.as_deref().map(safe_filter_token).unwrap_or(true),
        Recipe::SwiftReleaseBuild { product } => swift_release_product_allowed(product),
        Recipe::SwiftStackerBenchmarkRun {
            suite, report_root, ..
        } => safe_json_path_token(suite) && safe_report_root_token(report_root),
        Recipe::SwiftTest { filter } => filter.as_deref().map(safe_filter_token).unwrap_or(true),
        Recipe::Ripgrep {
            pattern,
            paths,
            max_matches,
        } => {
            !pattern.trim().is_empty()
                && *max_matches <= 1_000
                && paths.iter().all(|path| safe_relative_path_token(path))
        }
        Recipe::LocalSmoke { binary, args } => {
            safe_binary_name(binary)
                && !args.is_empty()
                && args.iter().all(|arg| arg.starts_with("--local-"))
        }
        _ => true,
    }
}

pub(crate) fn recipe_spec_to_command(spec: RecipeSpec) -> VerificationCommand {
    let (program, args) = match &spec.recipe {
        Recipe::CargoCheck => ("cargo".to_string(), vec!["check".to_string()]),
        Recipe::CargoBuild => ("cargo".to_string(), vec!["build".to_string()]),
        Recipe::CargoTestAll => ("cargo".to_string(), vec!["test".to_string()]),
        Recipe::CargoTestNamed { filter } => (
            "cargo".to_string(),
            vec!["test".to_string(), filter.clone()],
        ),
        Recipe::CargoMetadata => (
            "cargo".to_string(),
            vec![
                "metadata".to_string(),
                "--format-version".to_string(),
                "1".to_string(),
                "--no-deps".to_string(),
                "--locked".to_string(),
                "--offline".to_string(),
            ],
        ),
        Recipe::CargoRustcMetadata { package } => (
            "cargo".to_string(),
            vec![
                "rustc".to_string(),
                "-p".to_string(),
                package.clone(),
                "--lib".to_string(),
                "--locked".to_string(),
                "--offline".to_string(),
                "--".to_string(),
                "--emit=metadata".to_string(),
            ],
        ),
        Recipe::CargoRustdocPublic { package } => (
            "cargo".to_string(),
            vec![
                "doc".to_string(),
                "-p".to_string(),
                package.clone(),
                "--lib".to_string(),
                "--locked".to_string(),
                "--offline".to_string(),
                "--no-deps".to_string(),
            ],
        ),
        Recipe::CargoCheckPackage { package } => (
            "cargo".to_string(),
            vec![
                "check".to_string(),
                "-p".to_string(),
                package.clone(),
                "--locked".to_string(),
                "--offline".to_string(),
            ],
        ),
        Recipe::CargoTestPackage { package, filter } => {
            let mut args = vec![
                "test".to_string(),
                "-p".to_string(),
                package.clone(),
                "--locked".to_string(),
                "--offline".to_string(),
            ];
            if let Some(filter) = filter {
                args.push(filter.clone());
            }
            ("cargo".to_string(), args)
        }
        Recipe::CargoClippy => ("cargo".to_string(), vec!["clippy".to_string()]),
        Recipe::CargoFmtCheck => (
            "cargo".to_string(),
            vec!["fmt".to_string(), "--".to_string(), "--check".to_string()],
        ),
        Recipe::NpmTest => ("npm".to_string(), vec!["test".to_string()]),
        Recipe::NpmRunTest => (
            "npm".to_string(),
            vec!["run".to_string(), "test".to_string()],
        ),
        Recipe::NpmRunBuild => (
            "npm".to_string(),
            vec!["run".to_string(), "build".to_string()],
        ),
        Recipe::NpmWorkspaceTest { package } => (
            "npm".to_string(),
            vec![
                "test".to_string(),
                "--workspace".to_string(),
                package.clone(),
                "--if-present".to_string(),
            ],
        ),
        Recipe::PnpmTest => ("pnpm".to_string(), vec!["test".to_string()]),
        Recipe::PnpmBuild => ("pnpm".to_string(), vec!["build".to_string()]),
        Recipe::PnpmFilterTest { package } => (
            "pnpm".to_string(),
            vec!["--filter".to_string(), package.clone(), "test".to_string()],
        ),
        Recipe::YarnTest => ("yarn".to_string(), vec!["test".to_string()]),
        Recipe::YarnBuild => ("yarn".to_string(), vec!["build".to_string()]),
        Recipe::YarnWorkspaceTest { package } => (
            "yarn".to_string(),
            vec!["workspace".to_string(), package.clone(), "test".to_string()],
        ),
        Recipe::Pytest { args } => ("pytest".to_string(), args.clone()),
        Recipe::PythonPytest { args } => {
            let mut expanded = vec!["-m".to_string(), "pytest".to_string()];
            expanded.extend(args.iter().cloned());
            ("python".to_string(), expanded)
        }
        Recipe::SwiftBuild { product } => {
            let mut args = vec!["build".to_string()];
            if let Some(product) = product {
                args.extend(["--product".to_string(), product.clone()]);
            }
            ("swift".to_string(), args)
        }
        Recipe::SwiftReleaseBuild { product } => (
            "swift".to_string(),
            vec![
                "build".to_string(),
                "-c".to_string(),
                "release".to_string(),
                "--product".to_string(),
                product.clone(),
            ],
        ),
        Recipe::SwiftStackerBenchmarkRun {
            suite,
            report_root,
            generate_gallery,
        } => {
            let mut args = vec![
                "run".to_string(),
                "-c".to_string(),
                "release".to_string(),
                "SwiftStackerBench".to_string(),
                "--suite".to_string(),
                suite.clone(),
                "--report-root".to_string(),
                report_root.clone(),
            ];
            if *generate_gallery {
                args.push("--generate-gallery".to_string());
            }
            ("swift".to_string(), args)
        }
        Recipe::SwiftTest { filter } => {
            let mut args = vec!["test".to_string()];
            if let Some(filter) = filter {
                args.extend(["--filter".to_string(), filter.clone()]);
            }
            ("swift".to_string(), args)
        }
        Recipe::SwiftVersion => ("swift".to_string(), vec!["--version".to_string()]),
        Recipe::SwiftDumpPackage => (
            "swift".to_string(),
            vec!["package".to_string(), "dump-package".to_string()],
        ),
        Recipe::GitDiffCheck => (
            "git".to_string(),
            vec!["diff".to_string(), "--check".to_string()],
        ),
        Recipe::GitStatusShort => (
            "git".to_string(),
            vec!["status".to_string(), "--short".to_string()],
        ),
        Recipe::GitDiffStat => (
            "git".to_string(),
            vec!["diff".to_string(), "--stat".to_string()],
        ),
        Recipe::Ripgrep { pattern, paths, .. } => {
            let mut args = vec!["-n".to_string(), pattern.clone()];
            args.extend(paths.iter().cloned());
            ("rg".to_string(), args)
        }
        Recipe::LocalSmoke { binary, args } => (format!(".build/debug/{binary}"), args.clone()),
    };
    let mut command = VerificationCommand::new(program, args, &spec.source);
    command.id = spec.id;
    command.cwd = spec.cwd;
    command.timeout_seconds = spec.timeout_seconds;
    command.kind = spec.kind;
    command.failure_policy = spec.failure_policy;
    command.expected_exit = spec.expected_exit;
    command.tags = vec![format!("{:?}", spec.recipe)];
    command
}

pub(crate) fn is_deterministic_command_structured_allowed(command: &VerificationCommand) -> bool {
    if command.program.trim().is_empty()
        || command.args.iter().any(|arg| arg.contains('\0'))
        || contains_shell_control_operator(&command.program)
        || !verification_command_cwd_allowed(command.cwd.as_deref())
        || command.expected_exit.unwrap_or(0) != 0
        || command.timeout_seconds.unwrap_or(900) > 3_600
    {
        return false;
    }
    if crate::verification::ca::is_ca_verification_command(command) {
        return true;
    }
    match command.program.as_str() {
        "git" => matches!(
            command.args.as_slice(),
            [cmd, flag]
                if (cmd == "diff" && matches!(flag.as_str(), "--check" | "--stat"))
                    || (cmd == "status" && flag == "--short")
        ),
        "rg" => {
            !command.args.is_empty()
                && !command.args.iter().any(|arg| {
                    matches!(
                        arg.as_str(),
                        "--pre" | "--pre-glob" | "--hostname-bin" | "-f" | "--file" | "--generate"
                    ) || arg.starts_with("--pre=")
                        || arg.starts_with("--hostname-bin=")
                })
        }
        "cargo" => cargo_command_allowed(&command.args),
        // Compiler provenance is fixed metadata, never a source compilation or
        // a caller-selected tool invocation. It still uses command isolation.
        "rustc" => matches!(command.args.as_slice(), [version] if version == "-vV"),
        "swift" => is_swift_command_structured_allowed(command),
        "npm" => {
            matches!(
                command.args.as_slice(),
                [test] if test == "test"
            ) || matches!(
                command.args.as_slice(),
                [run, script] if run == "run" && matches!(script.as_str(), "test" | "build")
            ) || matches!(
                command.args.as_slice(),
                [test, workspace, package, present]
                    if test == "test"
                        && workspace == "--workspace"
                        && safe_package_token(package)
                        && present == "--if-present"
            )
        }
        "pnpm" => {
            matches!(
                command.args.as_slice(),
                [script] if matches!(script.as_str(), "test" | "build")
            ) || matches!(
                command.args.as_slice(),
                [filter, package, test]
                    if filter == "--filter" && safe_package_token(package) && test == "test"
            )
        }
        "yarn" => {
            matches!(
                command.args.as_slice(),
                [script] if matches!(script.as_str(), "test" | "build")
            ) || matches!(
                command.args.as_slice(),
                [workspace, package, test]
                    if workspace == "workspace" && safe_package_token(package) && test == "test"
            )
        }
        "pytest" => safe_pytest_owned_args(&command.args),
        "python" => {
            command
                .args
                .starts_with(&["-m".to_string(), "pytest".to_string()])
                && safe_pytest_owned_args(&command.args[2..])
        }
        _ => is_local_debug_smoke_command_structured_allowed(command),
    }
}

fn cargo_command_allowed(args: &[String]) -> bool {
    let shape_allowed = match args {
        [metadata, format, version, no_deps, locked, offline]
            if metadata == "metadata"
                && format == "--format-version"
                && version == "1"
                && no_deps == "--no-deps"
                && locked == "--locked"
                && offline == "--offline" =>
        {
            true
        }
        [command, package_flag, package, locked, offline]
            if matches!(command.as_str(), "test" | "check")
                && package_flag == "-p"
                && safe_package_token(package)
                && locked == "--locked"
                && offline == "--offline" =>
        {
            true
        }
        [test, package_flag, package, locked, offline, filter]
            if test == "test"
                && package_flag == "-p"
                && safe_package_token(package)
                && locked == "--locked"
                && offline == "--offline"
                && safe_filter_token(filter) =>
        {
            true
        }
        [rustc, package_flag, package, lib, locked, offline, separator, emit]
            if rustc == "rustc"
                && package_flag == "-p"
                && safe_package_token(package)
                && lib == "--lib"
                && locked == "--locked"
                && offline == "--offline"
                && separator == "--"
                && emit == "--emit=metadata" =>
        {
            true
        }
        [doc, package_flag, package, lib, locked, offline, no_deps]
            if doc == "doc"
                && package_flag == "-p"
                && safe_package_token(package)
                && lib == "--lib"
                && locked == "--locked"
                && offline == "--offline"
                && no_deps == "--no-deps" =>
        {
            true
        }
        [command, ..] if matches!(command.as_str(), "test" | "check" | "build" | "clippy") => true,
        [fmt, separator, check] if fmt == "fmt" && separator == "--" && check == "--check" => true,
        [fmt, check] if fmt == "fmt" && check == "--check" => true,
        _ => false,
    };
    shape_allowed
        && !args.iter().any(|arg| {
            arg == "--manifest-path"
                || arg.starts_with("--manifest-path=")
                || arg == "--target-dir"
                || arg.starts_with("--target-dir=")
                || arg == "-C"
                || arg.starts_with('/')
        })
}

fn safe_pytest_owned_args(args: &[String]) -> bool {
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    safe_pytest_args(&refs)
}

fn safe_pytest_args(args: &[&str]) -> bool {
    args.iter().all(|arg| {
        !arg.starts_with('/')
            && !arg.contains('\0')
            && *arg != "-c"
            && !arg.starts_with("-c=")
            && *arg != "--rootdir"
            && !arg.starts_with("--rootdir=")
            && *arg != "--basetemp"
            && !arg.starts_with("--basetemp=")
    })
}

pub(crate) fn recipe_kind(recipe: &Recipe) -> CommandKind {
    match recipe {
        Recipe::CargoCheck
        | Recipe::CargoCheckPackage { .. }
        | Recipe::CargoRustcMetadata { .. }
        | Recipe::CargoRustdocPublic { .. }
        | Recipe::CargoBuild
        | Recipe::NpmRunBuild
        | Recipe::PnpmBuild
        | Recipe::YarnBuild
        | Recipe::SwiftBuild { .. }
        | Recipe::SwiftReleaseBuild { .. } => CommandKind::Build,
        Recipe::SwiftStackerBenchmarkRun { .. } | Recipe::LocalSmoke { .. } => CommandKind::Smoke,
        Recipe::CargoClippy | Recipe::CargoFmtCheck => CommandKind::Lint,
        Recipe::CargoTestAll
        | Recipe::CargoTestNamed { .. }
        | Recipe::CargoTestPackage { .. }
        | Recipe::NpmTest
        | Recipe::NpmRunTest
        | Recipe::NpmWorkspaceTest { .. }
        | Recipe::PnpmTest
        | Recipe::PnpmFilterTest { .. }
        | Recipe::YarnTest
        | Recipe::YarnWorkspaceTest { .. }
        | Recipe::Pytest { .. }
        | Recipe::PythonPytest { .. }
        | Recipe::SwiftTest { .. } => CommandKind::Test,
        Recipe::SwiftVersion | Recipe::CargoMetadata | Recipe::SwiftDumpPackage => {
            CommandKind::Unknown
        }
        Recipe::GitDiffCheck | Recipe::GitStatusShort | Recipe::GitDiffStat => CommandKind::Git,
        Recipe::Ripgrep { .. } => CommandKind::Search,
    }
}

fn recipe_id(spec: &RecipeSpec) -> String {
    let text = serde_json::to_string(&spec.recipe).unwrap_or_else(|_| "recipe".to_string());
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("recipe:{hash:016x}")[..19].to_string()
}

fn safe_filter_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':'))
}

fn safe_package_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 128
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '@' | '/'))
        && !token.starts_with('/')
        && !token.contains("..")
}

fn safe_relative_path_token(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}

fn safe_json_path_token(path: &str) -> bool {
    safe_relative_path_token(path) && path.ends_with(".json")
}

fn safe_report_root_token(path: &str) -> bool {
    safe_relative_path_token(path)
        && path.split('/').next() == Some("Benchmarks")
        && path
            .split('/')
            .nth(1)
            .is_some_and(|second| second.starts_with("Reports"))
}

fn swift_release_product_allowed(product: &str) -> bool {
    product == "SwiftStackerBench"
}

fn safe_binary_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn is_swift_command_structured_allowed(command: &VerificationCommand) -> bool {
    match command.args.as_slice() {
        [package, dump] if package == "package" && dump == "dump-package" => true,
        [cmd] if cmd == "test" || cmd == "build" || cmd == "--version" => true,
        [cmd, flag, value] if cmd == "test" && flag == "--filter" && safe_filter_token(value) => {
            true
        }
        [cmd, flag, product]
            if cmd == "build" && flag == "--product" && safe_filter_token(product) =>
        {
            true
        }
        [cmd, config_flag, config, product_flag, product]
            if cmd == "build"
                && config_flag == "-c"
                && config == "release"
                && product_flag == "--product"
                && swift_release_product_allowed(product) =>
        {
            true
        }
        [cmd, config_flag, config, product, suite_flag, suite, report_flag, report_root]
            if cmd == "run"
                && config_flag == "-c"
                && config == "release"
                && product == "SwiftStackerBench"
                && suite_flag == "--suite"
                && report_flag == "--report-root"
                && safe_json_path_token(suite)
                && safe_report_root_token(report_root) =>
        {
            true
        }
        [cmd, config_flag, config, product, suite_flag, suite, report_flag, report_root, gallery_flag]
            if cmd == "run"
                && config_flag == "-c"
                && config == "release"
                && product == "SwiftStackerBench"
                && suite_flag == "--suite"
                && report_flag == "--report-root"
                && gallery_flag == "--generate-gallery"
                && safe_json_path_token(suite)
                && safe_report_root_token(report_root) =>
        {
            true
        }
        _ => false,
    }
}

fn verification_command_cwd_allowed(cwd: Option<&str>) -> bool {
    let Some(cwd) = cwd else {
        return true;
    };
    let cwd = cwd.trim();
    !cwd.is_empty()
        && !cwd.starts_with('/')
        && !cwd.contains('\\')
        && !cwd
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}

pub(crate) fn is_local_debug_smoke_command_structured_allowed(
    command: &VerificationCommand,
) -> bool {
    if command.args.len() != 1 || !command.args[0].starts_with("--local-") {
        return false;
    }
    command
        .program
        .strip_prefix(".build/debug/")
        .is_some_and(safe_binary_name)
}

fn split_command_words_strict(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = command.trim().chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            } else if ch == '\\' {
                return None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if ch.is_whitespace() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        if ch == '\\' || ch == '`' || ch == '$' && chars.peek() == Some(&'(') {
            return None;
        }
        if matches!(ch, ';' | '|' | '>' | '<' | '&') {
            return None;
        }
        current.push(ch);
    }
    if quote.is_some() {
        return None;
    }
    if !current.is_empty() {
        words.push(current);
    }
    (!words.is_empty()).then_some(words)
}

fn contains_shell_control_operator(command: &str) -> bool {
    split_command_words_strict(command).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(command: &str) -> VerificationCommand {
        parse_verification_command(command, "test").expect("command should parse")
    }

    #[test]
    fn cargo_format_and_clippy_reach_the_final_allowlist() {
        for command in ["cargo fmt -- --check", "cargo fmt --check", "cargo clippy"] {
            let command = parsed(command);
            assert!(is_deterministic_command_structured_allowed(&command));
        }
    }

    #[test]
    fn node_and_python_legacy_forms_match_the_structured_allowlist() {
        for command in [
            "npm test",
            "npm run test",
            "npm run build",
            "pnpm test",
            "pnpm build",
            "yarn test",
            "yarn build",
            "pytest tests/unit.py",
            "python -m pytest tests/unit.py",
        ] {
            let command = parsed(command);
            assert!(is_deterministic_command_structured_allowed(&command));
        }
    }

    #[test]
    fn registry_rejects_shells_and_authority_widening_arguments() {
        for command in [
            "cargo test && touch nope",
            "npm test | tee result",
            "python -m pytest --rootdir /tmp",
            "cargo test --manifest-path ../Cargo.toml",
        ] {
            assert!(parse_verification_command(command, "test").is_none());
        }
    }

    #[test]
    fn rust_toolchain_metadata_cannot_compile_or_load_caller_input() {
        let command = |args: &[&str]| {
            VerificationCommand::new(
                "rustc".into(),
                args.iter().map(|arg| (*arg).into()).collect(),
                "mechanistic.rust_api.v1",
            )
        };
        assert!(is_deterministic_command_structured_allowed(&command(&[
            "-vV"
        ])));
        for args in [
            vec!["src/lib.rs"],
            vec!["-vV", "src/lib.rs"],
            vec!["@args.txt"],
        ] {
            assert!(!is_deterministic_command_structured_allowed(&command(
                &args
            )));
        }
    }
}
