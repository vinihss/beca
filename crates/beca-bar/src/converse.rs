use super::*;

pub fn converse_spawn_or_signal(slot: &std::sync::Mutex<Option<std::process::Child>>) -> bool {
    let mut slot = match slot.lock() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut alive = false;
    if let Some(child) = slot.as_mut() {
        match child.try_wait() {
            Ok(None) => alive = true,
            Ok(Some(_)) => {
                *slot = None;
            }
            Err(_) => alive = true,
        }
    }
    if alive {
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            let path = std::path::PathBuf::from(dir)
                .join("beca")
                .join("converse.ctl");
            if let Ok(mut s) = std::os::unix::net::UnixStream::connect(&path) {
                use std::io::Write;
                let _ = s.write_all(b"quit\n");
            } else {
                eprintln!("[beca-bar] converse ctl unreachable: {}", path.display());
            }
        }
        return false;
    }

    if let Ok(exe) = std::env::current_exe() {
        let child = Command::new(exe)
            .env("BECA_NO_DAEMON", "1")
            .arg("mod")
            .arg("--local")
            .arg("voice:converse")
            .spawn();
        match child {
            Ok(c) => {
                *slot = Some(c);
                return true;
            }
            Err(e) => eprintln!("[beca-bar] failed to spawn voice:converse: {e}"),
        }
    }
    false
}
