//! Application discovery and launch (XDG .desktop entries).

pub mod desktop_entry;
pub mod indexador;

pub use desktop_entry::{clear_icon_cache, Application};
pub use indexador::{launch, reload_index, search};
