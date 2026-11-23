pub mod hyprland;
pub mod socket;

pub use socket::{run, send_request, socket_path, try_route_mod};
