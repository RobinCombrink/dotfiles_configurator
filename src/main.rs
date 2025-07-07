use anyhow::Result;
use clap::{Args, Parser};
use config::ConfigurationLoader;
use dirs::download_dir;
use env_logger;
use log::{error, trace, LevelFilter};
use reqwest::Client;
use std::env::home_dir;
use std::io::Write;
use std::path::PathBuf;
use std::process::exit;
use std::str::FromStr;

mod config;
mod dotfiles;
mod download;
mod github;
pub mod impls;
mod progress_bar;
mod shell_command;

const DEFAULT_DIRECTORY_PATH: &str = "config";

#[derive(Args, Debug, Clone)]
#[command(about = "Fetch config files from a remote github repository", long_about = None)]
struct RemoteConfigArguments {
    #[clap(short = 'o', long = "owner", default_value = "RobinCombrink")]
    owner: String,
    #[clap(short = 'r', long = "repo", default_value = "dotfiles")]
    repo: String,
    #[clap(
        short = 'p',
        long = "config-file-paths",
        default_value = "config/everywhere.dotconfig.json,config/personal.dotconfig.json",
        value_delimiter = ','
    )]
    config_file_paths: Vec<String>,
}

impl FromStr for RemoteConfigArguments {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() != 3 {
            return Err("Expected three comma-separated values".into());
        }
        Ok(RemoteConfigArguments {
            owner: parts[0].to_string(),
            repo: parts[1].to_string(),
            config_file_paths: parts[2].split(';').map(String::from).collect(),
        })
    }
}

#[derive(Args, Debug)]
#[command(
    about = "Fetch config files from multiple remote github repositories",
    long_about = "Fetch config files from multiple remote github repositories.\nSpecify each remote with a '--remote' flag. E.g., '--dotfiles remotes --remote owner,repo,config/Everywhere.json;config/Personal.json --remote different_owner,different_repo,config/different.json"
)]
struct RemotesConfigArguments {
    #[clap(
        short = 'r',
        long = "remote",
        default_value = "RobinCombrink,dotfiles,config/test.dotconfig.json"
    )]
    remotes: Vec<RemoteConfigArguments>,
}

#[derive(Args, Debug)]
#[command(about = "Use local config files", long_about = None)]
struct LocalConfigArguments {
    #[clap(short = 'd', long = "directory-path", default_value = DEFAULT_DIRECTORY_PATH)]
    directory_path: PathBuf,
}

#[derive(Args, Debug)]
#[command(about = "Provide config files both locally and from a remote github repository", long_about = None)]
struct AllConfigArguments {
    #[clap(flatten)]
    remote: RemoteConfigArguments,
    #[clap(flatten)]
    local: LocalConfigArguments,
}
#[derive(Parser, Debug)]
enum Command {
    Local(LocalConfigArguments),
    Remote(RemoteConfigArguments),
    Remotes(RemotesConfigArguments),
    All(AllConfigArguments),
}

#[derive(Parser, Debug)]
#[clap(name = "Dotfiles")]
#[command(version, about, long_about = None)]
struct Arguments {
    #[command(subcommand)]
    command: Command,
    #[arg(
        global = true,
        long("dry-run"),
        help = "Display an execution plan instead of executing on the plan"
    )]
    dry_run: bool,
}

#[tokio::main]
async fn main() {
    let args = Arguments::parse();

    match args.dry_run {
        true => setup_logging(LevelFilter::Error),
        false => setup_logging(LevelFilter::Info),
    }

    trace!("Logging setup successful");

    let config_loader = ConfigurationLoader::new(args.command);
    match config_loader.load_all_configurations().await {
        Ok(execution_plan) => {
            if args.dry_run {
                println!("{:#?}", execution_plan);
            } else {
                let client = Client::default();
                execution_plan
                    .execute(client)
                    .await
                    .into_iter()
                    .for_each(|applied_config| {
                        if let Err(err) = applied_config {
                            error!("Could not apply configuration item: {:?}", err)
                        }
                    });
            }
        }
        Err(err) => {
            println!("There was an error loading a configuration: {:?}", err);
            exit(1);
        }
    }
}

fn setup_logging(level_filter: LevelFilter) {
    env_logger::builder()
        .filter_level(level_filter)
        .parse_default_env()
        .format(|buf, record| {
            writeln!(
                buf,
                "{}:{} {} [{}] - {}",
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                chrono::Local::now().format("%H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .init();
}
