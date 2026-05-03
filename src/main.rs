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
    if updater::command_mode()? {
        return Ok(());
    }
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--capture") => {
            let path = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Expected absolute PNG path"))?;
            let screen = args.get(3).map_or("startup", String::as_str);
            let scale = args.get(4).map_or(Ok(1.), |s| s.parse::<f32>())?;
            let width = args.get(5).map_or(Ok(1120), |s| s.parse::<u32>())?;
            let height = args.get(6).map_or(Ok(760), |s| s.parse::<u32>())?;
            capture::run(std::path::Path::new(path), screen, [width, height], scale)
        }
        Some("--verify-rendering") => software::verify(
            args.get(2)
                .ok_or_else(|| anyhow::anyhow!("Expected report path"))?
                .into(),
        ),
        Some("--inventory") => {
            let report = windows_sources::scan()?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        None | Some("--updated") | Some("--cleanup-update") => {
            let Some(_instance) = Instance::acquire()? else {
                return Ok(());
            };
            software::run(None, None)
        }
        _ => anyhow::bail!("Unknown arguments"),
    }
}

struct Instance(windows_sys::Win32::Foundation::HANDLE);
impl Instance {
    fn acquire() -> anyhow::Result<Option<Self>> {
        use windows_sys::Win32::{
            Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
            System::Threading::CreateMutexW,
            UI::WindowsAndMessaging::*,
        };
        let identity = windows_sources::hash(settings::directory().to_string_lossy().as_bytes());
        let name: Vec<u16> = format!("Local\\StartupManager-{identity}\0")
            .encode_utf16()
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        anyhow::ensure!(
            !handle.is_null(),
            "Cannot acquire the application instance lock"
        );
        let instance = Self(handle);
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            let title: Vec<u16> = "StartupManager\0".encode_utf16().collect();
            let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
            if !window.is_null() {
                unsafe {
                    ShowWindow(window, SW_RESTORE);
                    SetForegroundWindow(window);
                }
            }
            return Ok(None);
        }
        Ok(Some(instance))
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
