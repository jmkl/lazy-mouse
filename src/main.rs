#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// The Windows-only code uses most of config/brush; on other platforms only the tests do.
#![cfg_attr(not(windows), allow(dead_code))]
mod brush;
mod config;
mod gui;
mod render;
mod rope;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.get(1).map(String::as_str) == Some("--gui") {
        // --gui <main_thread_id> <radius> <friction> <stiffness> <damping> <stretch>
        let tid: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        gui::run(tid, config::Settings::from_args(a.into_iter().skip(3)));
        return Ok(());
    }
    // Usage: lazy-mouse [radius] [friction] [stiffness] [damping] [stretch]
    win::run(config::Settings::from_args(std::env::args().skip(1)))
}

#[cfg(not(windows))]
fn main() {
    eprintln!("lazy-mouse only runs on Windows.");
}
