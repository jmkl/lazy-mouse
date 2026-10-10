fn main() {
    // Only run this build script if we are targeting Windows
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico"); // Replace with your icon's filename
        res.compile().unwrap();
    }
}
