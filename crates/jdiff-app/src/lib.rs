mod app;
mod ui;

pub use app::{JdiffApp, ViewMode};

use color_eyre::Result;
use ratatui::{Terminal, backend::Backend};
use std::path::PathBuf;

pub async fn run(start: Option<PathBuf>) -> Result<()> {
    jones_terminal::install_panic_hook();
    let mut app = JdiffApp::new(start)?;
    let mut terminal = jones_terminal::setup_terminal()?;
    let result = app.run(&mut terminal).await;
    jones_terminal::restore_terminal(&mut terminal)?;
    result
}

pub async fn run_with_terminal<B>(start: Option<PathBuf>, terminal: &mut Terminal<B>) -> Result<()>
where
    B: Backend,
    B::Error: Send + Sync + 'static,
{
    let mut app = JdiffApp::new(start)?;
    app.run(terminal).await
}
