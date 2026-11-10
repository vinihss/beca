use mlua::{Function, Lua, Table};

slint::include_modules!();

const BEHAVIOR: &str = include_str!("../../scripts/dashboard_behavior.luau");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = DashboardWindow::new()?;
    let luau = Lua::new();

    let dashboard: Table = luau.load(BEHAVIOR).eval()?;
    let update_metrics: Function = dashboard.get("generate_metrics")?;

    let apply = {
        let weak = app.as_weak();
        let update_metrics = update_metrics.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                if let Ok(res) = update_metrics.call::<Table>(()) {
                    if let Ok(cpu) = res.get::<String>("cpu") {
                        ui.set_cpu_status(cpu.into());
                    }
                    if let Ok(disk) = res.get::<String>("disk") {
                        ui.set_disk_status(disk.into());
                    }
                    if let Ok(net) = res.get::<String>("network") {
                        ui.set_network_status(net.into());
                    }
                }
            }
        }
    };

    apply();
    app.on_refresh_triggered(apply);

    app.run()?;
    Ok(())
}
