pub mod app;
pub mod input;
pub mod theme;
pub mod ui;

pub use app::App;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    app::run()
}
