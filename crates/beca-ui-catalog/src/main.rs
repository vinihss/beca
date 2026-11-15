slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = CatalogWindow::new()?;
    ui.run()
}
