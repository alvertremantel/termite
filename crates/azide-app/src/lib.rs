pub mod app;
pub mod draw;
pub mod explore;
pub mod feed_store;
pub mod settings;
pub mod ui;

pub use app::{AzideApp, AzideCustom, AzideEvent, ExploreFocus, RssView, SidebarItem};

use color_eyre::Result;
use ratatui::{Terminal, backend::Backend};

pub async fn run_tui() -> Result<()> {
    jones_terminal::install_panic_hook();
    let mut terminal = jones_terminal::setup_terminal()?;
    let result = run_tui_with_terminal(&mut terminal).await;
    jones_terminal::restore_terminal(&mut terminal)?;
    result
}

pub async fn run_tui_with_terminal<B>(terminal: &mut Terminal<B>) -> Result<()>
where
    B: Backend,
    B::Error: Send + Sync + 'static,
{
    let mut app = AzideApp::new().await?;
    app.run(terminal).await
}
