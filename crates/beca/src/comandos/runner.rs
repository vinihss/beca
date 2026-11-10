//! Script runner (`beca run`).

use anyhow::{Context, Result};
use std::path::Path;

pub fn run_script(script: &str, args: &[String]) -> Result<()> {
    let path = Path::new(script);
    let code = if path.is_file() {
        std::fs::read_to_string(path)
            .with_context(|| format!("failed to read script: {}", path.display()))?
    } else {
        // try XDG scripts dir by name
        let paths = beca_core::config::BecaPaths::resolve();
        let name = if script.ends_with(".lua") || script.ends_with(".luau") {
            script.to_string()
        } else {
            format!("{script}.lua")
        };
        let disk = paths.scripts_dir.join(&name);
        std::fs::read_to_string(&disk).with_context(|| {
            format!(
                "script not found: '{}' (tried file and {})",
                script,
                disk.display()
            )
        })?
    };

    let lua = mlua::Lua::new();
    beca_core::lua::bridge::install_beca_base(&lua).map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // arg table (1-based, like Lua)
    let arg_tbl = lua
        .create_table()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    arg_tbl
        .set(0, script)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    for (i, a) in args.iter().enumerate() {
        arg_tbl
            .set(i + 1, a.as_str())
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    }
    lua.globals()
        .set("arg", arg_tbl)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // set module context for reload (best-effort name)
    if let Ok(beca) = lua.globals().get::<mlua::Table>("beca") {
        if let Ok(rt) = beca.get::<mlua::Table>("runtime") {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(script)
                .to_string();
            let _ = rt.set("_module", name);
            let _ = rt.set("_embedded", code.as_str());
            let _ = rt.set("_script_path", script);
        }
    }

    lua.load(&code)
        .set_name(script)
        .exec()
        .map_err(|e| anyhow::anyhow!("script failed: {e}"))?;
    beca_core::lua::runtime::call_hook(&lua, "on_load");
    Ok(())
}
