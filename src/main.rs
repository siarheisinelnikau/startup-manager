#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod branding;
mod capture;
mod engine;
mod icon_pixels;
mod model;
mod registry;
mod repaint;
mod settings;
mod software;
mod ui;
mod updater;
mod windows_sources;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    if updater::command_mode()? { return Ok(()); }
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--capture") => {
            let path = args.get(2).ok_or_else(|| anyhow::anyhow!("Expected absolute PNG path"))?;
            let screen = args.get(3).map_or("startup", String::as_str);
            let scale = args.get(4).map_or(Ok(1.), |s| s.parse::<f32>())?;
            capture::run(std::path::Path::new(path), screen, [1120, 760], scale)
        }
        Some("--verify-rendering") => software::verify(args.get(2).ok_or_else(|| anyhow::anyhow!("Expected report path"))?.into()),
        Some("--demo") => software::demo(),
        Some("--inventory") => {
            let report = windows_sources::scan()?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        None | Some("--updated") | Some("--cleanup-update") => software::run(None, None),
        _ => anyhow::bail!("Unknown arguments"),
    }
}
