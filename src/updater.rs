use anyhow::{Context, Result};
use rust_auto_updator::{Config, Controller};

pub fn config() -> Result<Config> {
    let mut config = Config::new(
        "siarheisinelnikau",
        "startup-manager",
        crate::VERSION,
        "StartupManager",
    )
    .map_err(anyhow::Error::msg)?;
    config.expected_subsystem = Some(2);
    config.install.restart_args = vec!["--updated".into()];
    Ok(config)
}
pub fn start(ctx: egui::Context, preview: bool) -> Controller {
    if preview {
        return Controller::disabled(crate::VERSION, "Update checks are disabled in demo mode.");
    }
    config()
        .and_then(|config| {
            Controller::new(config, move || ctx.request_repaint()).map_err(anyhow::Error::msg)
        })
        .unwrap_or_else(|error| Controller::disabled(crate::VERSION, &error.to_string()))
}
pub fn command_mode() -> Result<bool> {
    if std::env::args().nth(1).as_deref() == Some("--apply-update") {
        rust_auto_updator::apply_update(&config()?).map_err(anyhow::Error::msg)?;
        return Ok(true);
    }
    Ok(false)
}
pub fn cleanup_pending() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(position) = args.iter().position(|a| a == "--cleanup-update")
        && let Some(path) = args.get(position + 1)
        && let Ok(config) = config()
    {
        rust_auto_updator::cleanup_after_update(path.into(), config.install);
    }
}
pub fn install(controller: &mut Controller) -> Result<()> {
    let mut prepared = controller.take_prepared().context("No prepared update")?;
    rust_auto_updator::handoff(&mut prepared).map_err(anyhow::Error::msg)
}
