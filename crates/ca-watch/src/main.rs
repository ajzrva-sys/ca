use std::process::ExitCode;

use ca_watch::{
    HostSpec, ProcessSpec, WatchRequestV1, WatchService, WatchState, DEFAULT_INTERVAL_SECS,
};
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "ca-watch",
    version,
    about = "Observe local or SSH-hosted processes"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Watch(WatchArgs),
}

#[derive(Debug, Args)]
struct WatchArgs {
    #[command(subcommand)]
    host: WatchHost,
}

#[derive(Debug, Subcommand)]
enum WatchHost {
    Local(ProcessArgs),
    Ssh(SshArgs),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Kind {
    Process,
    Copy,
}

#[derive(Debug, Args)]
struct ProcessArgs {
    #[arg(long)]
    pid: u32,
    #[arg(long, value_enum, default_value = "process")]
    kind: Kind,
    #[arg(long)]
    label: Option<String>,
    #[arg(long)]
    log: Option<String>,
    #[arg(long)]
    completion_marker: Option<String>,
    #[arg(long, default_value_t = DEFAULT_INTERVAL_SECS)]
    interval_secs: u64,
    #[arg(long)]
    jsonl: bool,
}

#[derive(Debug, Args)]
struct SshArgs {
    #[arg(long)]
    destination: String,
    #[arg(long)]
    port: Option<u16>,
    #[command(flatten)]
    process: ProcessArgs,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("ca-watch: {error}");
            ExitCode::from(2)
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode, String> {
    let (host, args) = match cli.command {
        Command::Watch(WatchArgs {
            host: WatchHost::Local(args),
        }) => (HostSpec::Local, args),
        Command::Watch(WatchArgs {
            host:
                WatchHost::Ssh(SshArgs {
                    destination,
                    port,
                    process,
                }),
        }) => (HostSpec::Ssh { destination, port }, process),
    };
    let process = match args.kind {
        Kind::Process => {
            if args.log.is_some() || args.completion_marker.is_some() {
                return Err("--log and --completion-marker require --kind copy".to_string());
            }
            ProcessSpec::Generic
        }
        Kind::Copy => ProcessSpec::Copy {
            log_path: args.log,
            completion_marker: args
                .completion_marker
                .ok_or_else(|| "--kind copy requires --completion-marker".to_string())?,
        },
    };
    let request = WatchRequestV1 {
        label: args
            .label
            .unwrap_or_else(|| format!("{} {}", process.kind_name(), args.pid)),
        host,
        pid: args.pid,
        process,
        interval_secs: args.interval_secs,
    };
    request.validate().map_err(|error| error.to_string())?;

    let service = WatchService::new();
    let mut events = service.subscribe();
    let id = service
        .start(request)
        .await
        .map_err(|error| error.to_string())?;
    loop {
        tokio::select! {
            result = events.recv() => {
                let event = result.map_err(|_| "watch event stream closed".to_string())?;
                if event.watch_id != id {
                    continue;
                }
                if args.jsonl {
                    println!("{}", serde_json::to_string(&event).map_err(|error| error.to_string())?);
                } else {
                    println!("{}  {:<11} {}", event.observed_at.to_rfc3339(), event.state, event.summary);
                }
                if event.state.is_terminal() {
                    return Ok(if event.state == WatchState::Failed {
                        ExitCode::from(1)
                    } else {
                        ExitCode::SUCCESS
                    });
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|error| error.to_string())?;
                let _ = service.stop(id).await;
                return Ok(ExitCode::from(130));
            }
        }
    }
}
