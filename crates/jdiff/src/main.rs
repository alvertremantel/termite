use color_eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let arg = std::env::args().nth(1);
    if matches!(arg.as_deref(), Some("-h" | "--help")) {
        println!(
            "Usage: jdiff [PATH]\n\nOpen a live Git diff viewer for PATH or the current directory."
        );
        return Ok(());
    }
    let start = arg.map(std::path::PathBuf::from);
    jdiff_app::run(start).await
}
