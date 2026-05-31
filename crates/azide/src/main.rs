use azide_cli::Cli;
use clap::Parser;
use color_eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let args = Cli::parse();

    match args.command {
        Some(command) => azide_cli::run_cli(command).await,
        None => azide_app::run_tui().await,
    }
}
