pub mod bar;
pub mod launcher;
pub mod runner;
pub mod voice;

use anyhow::Result;

pub fn dispatch(module: &str, action: &str, args: &[String]) -> Result<()> {
    match module {
        "launcher" | "apps" => match action {
            "show" => launcher::show(),
            "serve" => launcher::serve(),
            "hide" => launcher::send_control("hide"),
            "stop" | "quit" => launcher::send_control("quit"),
            other => {
                anyhow::bail!("unknown launcher action: {other} (expected: show, serve, hide, stop)")
            }
        },
        "bar" => bar::dispatch(action, args),
        "voice" => voice::dispatch(action, args),
        other => anyhow::bail!("unknown module: {other} (available: bar, launcher, voice)"),
    }
}
