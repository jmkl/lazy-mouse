// The Windows-only code uses most of config/brush; on other platforms only the tests do.
#![cfg_attr(not(windows), allow(dead_code))]

mod brush;
mod config;
mod render;
mod rope;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    // Usage: lazy-mouse [radius] [friction] [stiffness] [damping] [stretch]
    win::run(config::Settings::from_args(std::env::args().skip(1)))
}

#[cfg(not(windows))]
fn main() {
    eprintln!("lazy-mouse only runs on Windows.");
}
