use mlua::{Lua, Table};
use std::fs;

slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = MainWindow::new()?;

    // Luau VM Configuration (behavior.luau at workspace root, per guide)
    let luau = Lua::new();
    let script_content = fs::read_to_string("../../behavior.luau")
        .unwrap_or_else(|_| "local App={} App.card_title='behavior.luau not found'; App.button_text='OK'; function App.on_button_clicked(msg) print(msg) end; return App".to_string());
    let app_table: Table = luau.load(&script_content).eval()?;

    // Injecting Data from Luau into Slint properties
    let title: String = app_table
        .get("card_title")
        .unwrap_or_else(|_| "Loading via Luau...".to_string());
    let btn_text: String = app_table
        .get("button_text")
        .unwrap_or_else(|_| "Click Here".to_string());
    app.set_card_title(title.into());
    app.set_button_text(btn_text.into());

    // Injecting Behavior from Luau into Slint callbacks
    let click_function: mlua::Function = app_table.get("on_button_clicked")?;
    app.on_button_clicked(move |ui_data| {
        let _: Result<(), _> = click_function.call(ui_data.to_string());
    });

    app.run()?;
    Ok(())
}
