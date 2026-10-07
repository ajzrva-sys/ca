//! Offline recall fixture evaluation; never opens the user's memory stores.
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "ca memory",
    about = "Evaluate memory recall against an explicit offline fixture"
)]
pub struct MemoryCli {
    #[arg(long, global = true, default_value = "default")]
    pub profile: String,
    #[command(subcommand)]
    pub command: MemoryCommand,
}

#[derive(Debug, Subcommand)]
pub enum MemoryCommand {
    Benchmark {
        fixture: PathBuf,
        #[arg(long, default_value_t = 5)]
        k: usize,
        /// Explicit synthetic semantic scores; never calls an embedding service.
        #[arg(long, conflicts_with = "embedding_runtime")]
        semantic_scores: Option<PathBuf>,
        #[arg(long, default_value_t = 0.7)]
        minimum_similarity: f64,
        /// Exact already registered local runtime; no selection or download.
        #[arg(long, requires = "embedding_model")]
        embedding_runtime: Option<String>,
        #[arg(long, requires = "embedding_runtime")]
        embedding_model: Option<String>,
        #[arg(long, default_value_t = 70, value_parser = clap::value_parser!(u8).range(0..=100))]
        minimum_similarity_percent: u8,
    },
}

pub async fn run(cli: MemoryCli) -> i32 {
    let MemoryCommand::Benchmark {
        fixture,
        k,
        semantic_scores,
        minimum_similarity,
        embedding_runtime,
        embedding_model,
        minimum_similarity_percent,
    } = cli.command;
    let result = if let (Some(runtime_id), Some(model_id)) = (embedding_runtime, embedding_model) {
        match (
            read_fixture(&fixture),
            crate::runtime::RuntimePaths::discover(),
            crate::runtime::ActiveProfile::parse(Some(&cli.profile)),
        ) {
            (Ok(dataset), Ok(paths), Ok(profile)) => {
                crate::memory::evaluate_local_recall_benchmark(
                    &dataset,
                    k,
                    &paths,
                    &profile,
                    &crate::config::LocalHybridMemoryConfig {
                        runtime_id,
                        model_id,
                        context_size: 2048,
                        startup_timeout_secs: 30,
                        minimum_similarity_percent,
                    },
                )
                .await
                .and_then(|report| {
                    serde_json::to_string_pretty(&report).map_err(|error| error.to_string())
                })
            }
            (Err(error), _, _) => Err(error),
            (_, Err(error), _) => Err(error.to_string()),
            (_, _, Err(error)) => Err(error.to_string()),
        }
    } else {
        evaluate(&fixture, k, semantic_scores.as_deref(), minimum_similarity)
    };
    match result {
        Ok(report) => {
            println!("{report}");
            0
        }
        Err(error) => {
            eprintln!("ca memory benchmark: {error}");
            2
        }
    }
}

fn read_fixture<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<T, String> {
    use std::io::Read;
    const MAX_FIXTURE_BYTES: u64 = 8 * 1024 * 1024;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("fixture unavailable: {error}"))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_FIXTURE_BYTES {
        return Err("fixture must be a regular JSON file of at most 8MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FIXTURE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_FIXTURE_BYTES {
        return Err("fixture grew beyond 8MiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("invalid fixture: {error}"))
}

fn evaluate(
    path: &std::path::Path,
    k: usize,
    semantic_scores: Option<&std::path::Path>,
    minimum_similarity: f64,
) -> Result<String, String> {
    let fixture: crate::memory::RecallBenchmarkDataset = read_fixture(path)?;
    let report = if let Some(path) = semantic_scores {
        let scores: Vec<Vec<(String, f64)>> = read_fixture(path)?;
        crate::memory::evaluate_recall_benchmark_measured(
            &fixture,
            k,
            Some((minimum_similarity, &scores)),
        )?
    } else {
        crate::memory::evaluate_recall_benchmark_measured(&fixture, k, None)?
    };
    serde_json::to_string_pretty(&report).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_benchmark_cli_reports_measured_ranking_latency_and_context_bytes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/memory_recall/ranking.json");
        let report: serde_json::Value =
            serde_json::from_str(&evaluate(&path, 5, None, 0.7).unwrap()).unwrap();
        let latency = &report["latency"];
        assert!(
            latency.is_object(),
            "offline CLI benchmark must report latency"
        );
        assert_eq!(latency["ranking"]["samples"], 9);
        assert!(latency["ranking"]["p95_ms"].is_number());
        let timings = latency["cases"].as_array().unwrap();
        for (index, case) in timings.iter().enumerate() {
            assert!(case["ranking_ms"].is_number());
            assert_eq!(
                case["selected_context_bytes"],
                report["cases"][index]["selected_bytes"]
            );
        }
        let scores = path.with_file_name("semantic_scores.json");
        let semantic: serde_json::Value =
            serde_json::from_str(&evaluate(&path, 5, Some(&scores), 0.7).unwrap()).unwrap();
        assert!(semantic["latency"]["ranking"]["p95_ms"].is_number());
        assert_eq!(semantic["latency"]["cases"].as_array().unwrap().len(), 9);
    }

    #[test]
    fn benchmark_command_parses_and_evaluates_only_explicit_fixture() {
        let args =
            MemoryCli::try_parse_from(["ca memory", "benchmark", "fixture.json", "--k", "3"])
                .unwrap();
        assert!(matches!(
            args.command,
            MemoryCommand::Benchmark { k: 3, .. }
        ));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/memory_recall/ranking.json");
        let report: serde_json::Value =
            serde_json::from_str(&evaluate(&path, 5, None, 0.7).unwrap()).unwrap();
        assert_eq!(report["k"], 5);
        assert!(report["positive_cases"].as_u64().unwrap() > 0);
        assert!(evaluate(&path, 0, None, 0.7).is_err());
        let scores = path.with_file_name("semantic_scores.json");
        assert!(evaluate(&path, 5, Some(&scores), 0.7).is_ok());
        assert!(evaluate(&path, 5, Some(&scores), 1.1).is_err());
        assert!(MemoryCli::try_parse_from([
            "ca memory",
            "benchmark",
            "fixture.json",
            "--embedding-runtime",
            "runtime"
        ])
        .is_err());
        assert!(MemoryCli::try_parse_from([
            "ca memory",
            "benchmark",
            "fixture.json",
            "--embedding-runtime",
            "runtime",
            "--embedding-model",
            "model",
            "--minimum-similarity-percent",
            "101"
        ])
        .is_err());
    }
}
