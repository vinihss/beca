use mlua::{Function, Lua, Table};

slint::include_modules!();

const BEHAVIOR: &str = include_str!("../../scripts/wizard_behavior.luau");

fn apply_view(ui: &WizardWindow, view: &Table) {
    if let Ok(title) = view.get::<String>("title") {
        ui.set_step_title(title.into());
    }
    if let Ok(desc) = view.get::<String>("desc") {
        ui.set_step_description(desc.into());
    }
    if let Ok(is_first) = view.get::<bool>("is_first") {
        ui.set_is_first_step(is_first);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = WizardWindow::new()?;
    let luau = Lua::new();
    let wizard: Table = luau.load(BEHAVIOR).eval()?;

    let get_current: Function = wizard.get("get_current_view")?;
    let on_next: Function = wizard.get("on_next")?;
    let on_back: Function = wizard.get("on_back")?;

    let initial: Table = get_current.call(())?;
    apply_view(&app, &initial);

    {
        let weak = app.as_weak();
        let on_next = on_next.clone();
        app.on_next_pressed(move || {
            if let Ok(view) = on_next.call::<Table>(()) {
                if let Some(ui) = weak.upgrade() {
                    apply_view(&ui, &view);
                }
            }
        });
    }
    {
        let weak = app.as_weak();
        app.on_back_pressed(move || {
            if let Ok(view) = on_back.call::<Table>(()) {
                if let Some(ui) = weak.upgrade() {
                    apply_view(&ui, &view);
                }
            }
        });
    }

    app.run()?;
    Ok(())
}
