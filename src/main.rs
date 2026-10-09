mod brush;
mod render;
mod rope;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    // Usage: lazy-mouse [radius_px] [friction 0..1]
    let arg = |i| std::env::args().nth(i).and_then(|a| a.parse::<f64>().ok());
    let radius = arg(1)
        .filter(|r| r.is_finite() && *r >= 0.0)
        .unwrap_or(40.0);
    let friction = arg(2)
        .filter(|f| f.is_finite())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    win::run(radius, friction)
}

#[cfg(not(windows))]
fn main() {
    eprintln!("lazy-mouse only runs on Windows.");
}
