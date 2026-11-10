use anyhow::{Context, Result};
use mlua::{Function, Lua, Table};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::Rc};

use ui_lib::SearchWindow;

const EMBEDDED_LAUNCHER: &str = include_str!("../../../../scripts/launcher.lua");

fn lua_err(e: mlua::Error) -> anyhow::Error {
    anyhow::anyhow!(e.to_string())
}

fn parse_hex_color(hex: &str) -> Option<slint::Color> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(slint::Color::from_rgb_u8(r, g, b))
}

fn apply_beca_theme(app: &SearchWindow) {
    let resolved = beca_core::load();
    let t = &resolved.theme;
    let g = ui_lib::Theme::get(app);
    let to_brush = |s: &str| -> slint::Brush {
        parse_hex_color(s)
            .map(slint::Brush::SolidColor)
            .unwrap_or_else(|| slint::Brush::SolidColor(slint::Color::from_rgb_u8(0, 0, 0)))
    };
    let to_color = |s: &str| -> slint::Color {
        parse_hex_color(s).unwrap_or_else(|| slint::Color::from_rgb_u8(0, 0, 0))
    };
    g.set_background(to_color(&t.colors.background));
    g.set_text_main(to_color(&t.colors.text_main));
    g.set_card_bg(to_color(&t.colors.card_bg));
    g.set_card_border(to_color(&t.colors.card_border));
    g.set_surface(to_brush(&t.colors.surface));
    g.set_surface_alt(to_brush(&t.colors.surface_alt));
    g.set_border(to_brush(&t.colors.border));
    g.set_text_strong(to_brush(&t.colors.text_strong));
    g.set_accent(to_brush(&t.colors.accent));
    g.set_accent_success(to_brush(&t.colors.accent_success));
    g.set_accent_danger(to_brush(&t.colors.accent_danger));
    g.set_recording(to_brush(&t.colors.recording));
    g.set_radius(t.metrics.radius as f32);
    g.set_dark_mode(t.meta.variant != "light");
}

pub fn show() -> Result<()> {
    let window = SearchWindow::new().context("could not create SearchWindow")?;
    apply_beca_theme(&window);
    window.hide().ok();

    let lua = Lua::new();
    let beca = crate::runtime::install_beca_base(&lua).map_err(lua_err)?;
    let search_slot: Rc<RefCell<Option<Table>>> = Rc::new(RefCell::new(None));

    // beca.apps
    let apps: Table = beca.get("apps").map_err(lua_err)?;
    apps.set(
        "search",
        lua.create_function(|lua, query: String| {
            let results = beca_apps::search(&query);
            let t = lua.create_table()?;
            for (i, app) in results.iter().enumerate() {
                t.set(i + 1, app.name.clone())?;
            }
            Ok(t)
        })
        .map_err(lua_err)?,
    )
    .map_err(lua_err)?;
    apps.set(
        "launch",
        lua.create_function(|_, name: String| {
            beca_apps::launch(&name)
                .map(|_| ())
                .map_err(mlua::Error::external)
        })
        .map_err(lua_err)?,
    )
    .map_err(lua_err)?;

    // beca.ui.search_window(opts)
    let ui: Table = beca.get("ui").map_err(lua_err)?;
    let weak = window.as_weak();
    let slot_for_create = search_slot.clone();
    ui.set(
        "search_window",
        lua.create_function(move |lua, options: Option<Table>| {
            if let Some(options) = options {
                if let Ok(placeholder) = options.get::<String>("placeholder") {
                    if let Some(w) = weak.upgrade() {
                        w.set_placeholder(placeholder.into());
                    }
                }
                let _ = options.get::<String>("title");
            }
            let search = lua.create_table()?;
            let weak_results = weak.clone();
            search.set(
                "set_results",
                lua.create_function(move |_, items: Table| {
                    let mut results = Vec::new();
                    for value in items.sequence_values::<String>() {
                        results.push(SharedString::from(value?));
                    }
                    if let Some(w) = weak_results.upgrade() {
                        w.set_results(ModelRc::new(VecModel::from(results)));
                    }
                    Ok(())
                })?,
            )?;
            let weak_show = weak.clone();
            search.set(
                "show",
                lua.create_function(move |_, ()| {
                    if let Some(w) = weak_show.upgrade() {
                        let _ = w.show();
                    }
                    Ok(())
                })?,
            )?;
            let weak_hide = weak.clone();
            search.set(
                "hide",
                lua.create_function(move |_, ()| {
                    if let Some(w) = weak_hide.upgrade() {
                        let _ = w.hide();
                    }
                    Ok(())
                })?,
            )?;
            *slot_for_create.borrow_mut() = Some(search.clone());
            Ok(search)
        })
        .map_err(lua_err)?,
    )
    .map_err(lua_err)?;

    // connect Slint events -> Lua callbacks
    {
        let slot = search_slot.clone();
        window.on_search_requested(move |query| {
            if let Some(search) = slot.borrow().clone() {
                if let Ok(cb) = search.get::<Function>("on_search") {
                    if let Err(e) = cb.call::<()>(query.to_string()) {
                        eprintln!("Lua on_search failed: {e}");
                    }
                }
            }
        });
    }
    {
        let slot = search_slot.clone();
        window.on_result_selected(move |item| {
            if let Some(search) = slot.borrow().clone() {
                if let Ok(cb) = search.get::<Function>("on_select") {
                    if let Err(e) = cb.call::<()>(item.to_string()) {
                        eprintln!("Lua on_select failed: {e}");
                    }
                }
            }
        });
    }

    crate::runtime::exec_module_script(&lua, "launcher", EMBEDDED_LAUNCHER)
        .context("launcher script failed")?;

    slint::run_event_loop().context("Slint event loop failed")?;
    Ok(())
}
