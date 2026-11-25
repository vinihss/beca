pub mod desktop_entry;
pub mod indexer;

pub use desktop_entry::{Application, clear_icon_cache};
pub use indexer::{launch, reload_index, search};
