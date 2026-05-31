pub mod app;
pub mod draw;
pub mod ui;

pub use app::TermiteApp;

use color_eyre::Result;
use std::path::PathBuf;

pub async fn run(maybe_path: Option<PathBuf>) -> Result<()> {
    jones_terminal::install_panic_hook();
    let mut terminal = jones_terminal::setup_terminal()?;
    let mut app = TermiteApp::new(maybe_path)?;
    let result = app.run(&mut terminal).await;
    jones_terminal::restore_terminal(&mut terminal)?;
    result
}
