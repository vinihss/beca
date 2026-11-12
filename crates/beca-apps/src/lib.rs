pub mod desktop_entry;
pub mod indexer;

pub use desktop_entry::{clear_icon_cache, Application};
pub use indexer::{launch, reload_index, search};
