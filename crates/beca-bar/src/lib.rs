//! Status bar implementation: layout, Hyprland geometry, widgets and rendering.

use anyhow::Result;
use beca_ui::ui_loader;
use slint_interpreter::{ComponentHandle, Value};
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

pub mod converse;
pub mod hyprland;
pub mod show;
pub mod widgets;

pub use hyprland::clear_reserved_for_position;
pub use show::show;
