//! Interactive personality setup and continuous local evidence review.
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Parser)]
#[command(
    name = "ca harness questionnaire",
    about = "Set personality preferences interactively and review permanent behavior"
)]
struct QuestionnaireCli {
    /// Isolated runtime profile to inspect or improve.
    #[arg(long, global = true, default_value = "default")]
    profile: String,
    /// Emit JSON; watch emits one JSON object per changed cycle.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<QuestionnaireCommand>,
}

#[derive(Debug, Subcommand)]
enum QuestionnaireCommand {
    /// Show every built-in behavior question and its evidence requirements.
    Show,
    /// Answer guided questions and save how CA should work and communicate.
    #[command(visible_alias = "edit")]
    Setup,
    /// Start guided setup; nonterminal use creates a personality template.
    Init,
    /// Show the active Markdown personality steering and its path.
    Steering,
    /// Review local outcomes once and persist the updated learning report.
    Run {
        /// Maximum recent outcome records examined in this cycle.
        #[arg(long, default_value_t = 200, value_parser = parse_limit)]
        limit: usize,
    },
    /// Keep reviewing local outcomes until Ctrl-C or the requested cycle count.
    Watch {
        /// Seconds between completed cycles.
        #[arg(long, visible_alias = "interval-seconds", default_value_t = 300, value_parser = parse_interval)]
        interval: u64,
        /// Stop after this many cycles; omitted means continuous operation.
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        cycles: Option<u64>,
        /// Maximum recent outcome records examined in each cycle.
        #[arg(long, default_value_t = 200, value_parser = parse_limit)]
        limit: usize,
    },
    /// Read the latest saved report without starting a cycle.
    Status,
}

fn parse_limit(value: &str) -> Result<usize, String> {
    let limit = value
        .parse::<usize>()
        .map_err(|_| "limit must be an integer from 1 through 1000".to_string())?;
    if (1..=1000).contains(&limit) {
        Ok(limit)
    } else {
        Err("limit must be an integer from 1 through 1000".into())
    }
}

fn parse_interval(value: &str) -> Result<u64, String> {
    let seconds = value
        .parse::<u64>()
        .map_err(|_| "interval must be a positive number of seconds".to_string())?;
    if seconds == 0 {
        return Err("interval must be at least 1 second".into());
    }
    if Instant::now()
        .checked_add(Duration::from_secs(seconds))
        .is_none()
    {
        return Err("interval exceeds the platform clock range".into());
    }
    Ok(seconds)
}

/// `args` begins after the `questionnaire` command, excluding the executable name.
pub async fn run_cli(args: &[OsString]) -> i32 {
    let cli = match QuestionnaireCli::try_parse_from(
        std::iter::once(OsString::from("ca harness questionnaire")).chain(args.iter().cloned()),
    ) {
        Ok(cli) => cli,
        Err(error) => {
            let code = error.exit_code();
            let _ = error.print();
            return code;
        }
    };
    match execute(cli).await {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ca harness questionnaire: {error:#}");
            1
        }
    }
}

async fn execute(cli: QuestionnaireCli) -> Result<()> {
    let profile = crate::runtime::ActiveProfile::parse(Some(&cli.profile))?;
    let interactive = !cli.json && crate::personality_questionnaire::interactive_terminal();
    let command = cli.command.unwrap_or(if interactive {
        QuestionnaireCommand::Setup
    } else {
        QuestionnaireCommand::Show
    });
    if matches!(command, QuestionnaireCommand::Show) {
        if cli.json {
            println!(
                "{}",
                serde_json::to_string_pretty(crate::harness_questionnaire_catalog::QUESTIONS)?
            );
        } else {
            println!(
                "{}",
                crate::harness_questionnaire_catalog::format_questionnaire()
            );
        }
        return Ok(());
    }
    if matches!(profile, crate::runtime::ActiveProfile::Bare) {
        anyhow::bail!("the bare profile does not persist questionnaire learning or steering");
    }
    let paths = crate::runtime::RuntimePaths::discover()?.for_profile(&profile);
    let data_dir = paths.profile_data_dir(&profile);
    match command {
        QuestionnaireCommand::Show => unreachable!(),
        QuestionnaireCommand::Setup => {
            if cli.json {
                anyhow::bail!("interactive setup does not use JSON output; omit --json");
            }
            crate::personality_questionnaire::run(paths, profile).await?;
        }
        QuestionnaireCommand::Init => {
            if interactive {
                return crate::personality_questionnaire::run(paths, profile).await;
            }
            let path = crate::harness_personality::initialize(&paths, &profile)?;
            if cli.json {
                println!("{}", serde_json::json!({ "path": path }));
            } else {
                println!(
                    "Personality ready: {}. Run ca personality to choose preferences.",
                    path.display()
                );
            }
        }
        QuestionnaireCommand::Steering => {
            let path = crate::harness_personality::path(&paths, &profile);
            let content = crate::harness_personality::read(&paths, &profile)?;
            let active_content = crate::harness_personality::load(&paths, &profile)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({ "path": path, "content": content, "active_content": active_content })
                );
            } else if let Some(content) = content {
                println!("{}\n\n{content}", path.display());
            } else {
                println!(
                    "No personality steering at {}. Run ca personality to choose preferences.",
                    path.display()
                );
            }
        }
        QuestionnaireCommand::Run { limit } => {
            let report = run_cycle(data_dir, limit).await?;
            print_report(&report, cli.json, false)?;
        }
        QuestionnaireCommand::Watch {
            interval,
            cycles,
            limit,
        } => watch(data_dir, limit, interval, cycles, cli.json).await?,
        QuestionnaireCommand::Status => {
            let report = tokio::task::spawn_blocking(move || {
                crate::harness_questionnaire::status(&data_dir)
            })
            .await
            .context("questionnaire status worker failed")??;
            if let Some(report) = report {
                print_report(&report, cli.json, false)?;
            } else if cli.json {
                println!("null");
            } else {
                println!("No questionnaire runs yet. Run ca harness questionnaire run.");
            }
        }
    }
    Ok(())
}

async fn run_cycle(
    data_dir: PathBuf,
    limit: usize,
) -> Result<crate::harness_questionnaire::Report> {
    tokio::task::spawn_blocking(move || crate::harness_questionnaire::run_cycle(&data_dir, limit))
        .await
        .context("questionnaire cycle worker failed")?
}

fn print_report(
    report: &crate::harness_questionnaire::Report,
    json: bool,
    streaming: bool,
) -> Result<()> {
    if json {
        let output = if streaming {
            serde_json::to_string(report)?
        } else {
            serde_json::to_string_pretty(report)?
        };
        println!("{output}");
    } else {
        println!("{}", crate::harness_questionnaire::format_report(report));
    }
    std::io::stdout()
        .flush()
        .context("could not flush questionnaire report")
}

async fn watch(
    data_dir: PathBuf,
    limit: usize,
    interval: u64,
    cycles: Option<u64>,
    json: bool,
) -> Result<()> {
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    let mut completed = 0u64;
    loop {
        let cycle_dir = data_dir.clone();
        let mut worker = tokio::task::spawn_blocking(move || {
            crate::harness_questionnaire::run_cycle(&cycle_dir, limit)
        });
        // A stopped watch finishes the one bounded write already in progress.
        // Dropping a blocking task would leave it mutating state after exit.
        let (report, stopped) = tokio::select! {
            // Poll shutdown first so its handler is installed before any report.
            biased;
            result = &mut shutdown => {
                result.context("could not listen for Ctrl-C")?;
                (worker.await.context("questionnaire cycle worker failed")??, true)
            }
            result = &mut worker => {
                (result.context("questionnaire cycle worker failed")??, false)
            }
        };
        if completed == 0 || report.changed {
            print_report(&report, json, true)?;
        }
        completed = completed.saturating_add(1);
        if stopped || cycles.is_some_and(|maximum| completed >= maximum) {
            return Ok(());
        }
        tokio::select! {
            result = &mut shutdown => {
                result.context("could not listen for Ctrl-C")?;
                return Ok(());
            }
            () = tokio::time::sleep(Duration::from_secs(interval)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<QuestionnaireCli, clap::Error> {
        QuestionnaireCli::try_parse_from(
            std::iter::once("ca harness questionnaire").chain(args.iter().copied()),
        )
    }

    #[test]
    fn bounded_watch_options_and_global_flags_parse() {
        let cli = parse(&[
            "watch",
            "--profile",
            "work",
            "--json",
            "--interval",
            "1",
            "--cycles",
            "2",
            "--limit",
            "1000",
        ])
        .unwrap();
        assert_eq!(cli.profile, "work");
        assert!(cli.json);
        assert!(matches!(
            cli.command,
            Some(QuestionnaireCommand::Watch {
                interval: 1,
                cycles: Some(2),
                limit: 1000
            })
        ));
        for args in [
            vec!["watch", "--interval", "0"],
            vec!["watch", "--cycles", "0"],
            vec!["run", "--limit", "0"],
            vec!["run", "--limit", "1001"],
            vec!["run", "--limit", "abc"],
        ] {
            assert_eq!(parse(&args).unwrap_err().exit_code(), 2, "{args:?}");
        }
        assert!(parse(&[]).unwrap().command.is_none());
        assert_eq!(parse(&["--help"]).unwrap_err().exit_code(), 0);
    }
}
