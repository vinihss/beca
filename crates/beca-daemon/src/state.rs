use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub use beca_process::is_pid_alive;

/// Processes supervised by the daemon.
#[derive(Default)]
pub struct DaemonState {
    pub bars: HashMap<String, u32>,
    pub launcher_pid: Option<u32>,
    pub voice_pid: Option<u32>,
}

/// Returns the process-wide daemon state.
pub fn state() -> &'static Mutex<DaemonState> {
    static S: OnceLock<Mutex<DaemonState>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(DaemonState::default()))
}
