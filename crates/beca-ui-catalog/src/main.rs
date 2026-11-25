use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = CatalogWindow::new()?;

    // The metric panel is a real window, not a scroll target. Keep it alive for the
    // whole catalog session; closing it only hides it and the icon shows it again.
    let panel: Rc<RefCell<Option<beca_ui::AppWindow>>> = Rc::new(RefCell::new(None));

    {
        let panel = panel.clone();
        ui.on_open_panel(move || {
            if let Some(existing) = panel.borrow().as_ref()
                && existing.show().is_ok()
            {
                return;
            }

            match beca_ui::AppWindow::new() {
                Ok(win) => {
                    let weak = win.as_weak();
                    win.on_close_requested(move || {
                        if let Some(win) = weak.upgrade() {
                            let _ = win.hide();
                        }
                    });
                    if let Err(err) = win.show() {
                        eprintln!("[catalog] could not show metric panel: {err}");
                        return;
                    }
                    panel.borrow_mut().replace(win);
                }
                Err(err) => eprintln!("[catalog] could not create metric panel: {err}"),
            }
        });
    }

    ui.run()
}
