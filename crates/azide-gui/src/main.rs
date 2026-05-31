use color_eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;
    azide_gui::run()
}
