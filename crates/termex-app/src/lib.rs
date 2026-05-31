mod app;
mod ui;

pub use app::{DocumentKind, Prompt, TermexApp, TermexMode};

use color_eyre::Result;
use std::path::PathBuf;

pub async fn run(maybe_path: Option<PathBuf>) -> Result<()> {
    jones_terminal::install_panic_hook();
    let mut app = TermexApp::new(maybe_path)?;
    let mut terminal = jones_terminal::setup_terminal()?;
    let result = app.run(&mut terminal).await;
    jones_terminal::restore_terminal(&mut terminal)?;
    result
}
