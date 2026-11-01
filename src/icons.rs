//! Native file icons, resolved once on a sleeping worker, never during painting.
use crate::model::{Entry, Source};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};
use windows_sys::Win32::{
    Graphics::Gdi::*,
    Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, GetDriveTypeW},
    System::{
        Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
        Environment::ExpandEnvironmentStringsW,
    },
    UI::{
        Shell::*,
        WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx},
    },
};

const SIZE: usize = 32;
#[derive(Clone)]
pub struct Pixels {
    pub rgba: Vec<u8>,
    pub resolved: bool,
}
struct Target {
    id: String,
    command: String,
    file: bool,
}
impl Target {
    fn from_entry(entry: &Entry) -> Self {
        let (command, file) = match &entry.source {
            Source::Folder { path, .. } => (path.clone(), true),
            _ => (entry.command.clone(), false),
        };
        Self {
            id: entry.id.clone(),
            command,
            file,
        }
    }
}
struct Job {
    generation: u64,
    targets: Vec<Target>,
}
struct Loaded {
    id: String,
    key: String,
    pixels: Arc<Pixels>,
}
struct Batch {
    generation: u64,
    icons: Vec<Loaded>,
}
struct Worker {
    sender: Sender<Job>,
    receiver: Receiver<Batch>,
    generation: Arc<AtomicU64>,
}
pub struct ApplicationIcons {
    worker: Option<Worker>,
    generation: u64,
    entries: HashMap<String, String>,
    textures: HashMap<String, egui::TextureHandle>,
    resolved: HashSet<String>,
}
impl ApplicationIcons {
    pub fn new(ctx: egui::Context, preview: bool) -> Self {
        let worker = if preview {
            None
        } else {
            let (sender, jobs) = mpsc::channel::<Job>();
            let (results, receiver) = mpsc::channel();
            let generation = Arc::new(AtomicU64::new(0));
            let latest = generation.clone();
            std::thread::spawn(move || {
                let _com = Com::new();
                let mut cache = HashMap::new();
                while let Ok(job) = jobs.recv() {
                    let mut icons = vec![];
                    for target in job.targets {
                        if latest.load(Ordering::Relaxed) != job.generation {
                            break;
                        }
                        icons.push(load(&target, &mut cache));
                        if icons.len() == 8 {
                            if results
                                .send(Batch {
                                    generation: job.generation,
                                    icons: std::mem::take(&mut icons),
                                })
                                .is_err()
                            {
                                return;
                            }
                            ctx.request_repaint();
                        }
                    }
                    if !icons.is_empty() {
                        if results
                            .send(Batch {
                                generation: job.generation,
                                icons,
                            })
                            .is_err()
                        {
                            return;
                        }
                        ctx.request_repaint();
                    }
                }
            });
            Some(Worker {
                sender,
                receiver,
                generation,
            })
        };
        Self {
            worker,
            generation: 0,
            entries: HashMap::new(),
            textures: HashMap::new(),
            resolved: HashSet::new(),
        }
    }
    pub fn request(&mut self, entries: &[Entry]) {
        let Some(worker) = &self.worker else {
            return;
        };
        self.generation += 1;
        worker.generation.store(self.generation, Ordering::Relaxed);
        self.entries
            .retain(|id, _| entries.iter().any(|entry| entry.id == *id));
        self.resolved
            .retain(|id| entries.iter().any(|entry| entry.id == *id));
        let _ = worker.sender.send(Job {
            generation: self.generation,
            targets: entries.iter().map(Target::from_entry).collect(),
        });
    }
    pub fn preview(&mut self, ctx: &egui::Context, entries: &[Entry]) {
        self.entries.clear();
        self.resolved.clear();
        let _com = Com::new();
        let mut cache = HashMap::new();
        for entry in entries {
            self.insert(ctx, load(&Target::from_entry(entry), &mut cache));
        }
    }
    pub fn receive(&mut self, ctx: &egui::Context) -> bool {
        let batches: Vec<_> = self
            .worker
            .as_ref()
            .map_or_else(Vec::new, |worker| worker.receiver.try_iter().collect());
        let mut changed = false;
        for batch in batches {
            if batch.generation == self.generation {
                for icon in batch.icons {
                    self.insert(ctx, icon);
                    changed = true;
                }
            }
        }
        if changed {
            self.textures
                .retain(|key, _| self.entries.values().any(|used| used == key));
        }
        changed
    }
    fn insert(&mut self, ctx: &egui::Context, icon: Loaded) {
        if icon.pixels.resolved {
            self.resolved.insert(icon.id.clone());
        } else {
            self.resolved.remove(&icon.id);
        }
        self.textures.entry(icon.key.clone()).or_insert_with(|| {
            ctx.load_texture(
                format!("file-icon:{}", icon.key),
                egui::ColorImage::from_rgba_unmultiplied([SIZE, SIZE], &icon.pixels.rgba),
                egui::TextureOptions::LINEAR,
            )
        });
        self.entries.insert(icon.id, icon.key);
    }
    pub fn texture(&self, id: &str) -> Option<&egui::TextureHandle> {
        self.entries.get(id).and_then(|key| self.textures.get(key))
    }
    pub fn resolved_count(&self) -> usize {
        self.resolved.len()
    }
}

fn load(target: &Target, cache: &mut HashMap<String, Arc<Pixels>>) -> Loaded {
    let path = if target.file {
        local_file(&target.command)
    } else {
        resolve_command(&target.command)
    };
    let key = path.as_ref().map_or_else(
        || "generic-application".into(),
        |path| {
            let metadata = std::fs::metadata(path).ok();
            format!(
                "{}:{:?}:{}",
                path.to_string_lossy().to_lowercase(),
                metadata.as_ref().and_then(|meta| meta.modified().ok()),
                metadata.map_or(0, |meta| meta.len())
            )
        },
    );
    if cache.len() > 256 {
        cache.clear();
    }
    let pixels = cache
        .entry(key.clone())
        .or_insert_with(|| {
            let actual = path.as_ref().and_then(|path| extract(path, false));
            Arc::new(actual.unwrap_or_else(|| {
                extract(Path::new("application.exe"), true).unwrap_or_else(fallback)
            }))
        })
        .clone();
    Loaded {
        id: target.id.clone(),
        key,
        pixels,
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn expanded(text: &str) -> String {
    let source = wide(text);
    let mut buffer = vec![0u16; 32768];
    let count = unsafe {
        ExpandEnvironmentStringsW(source.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32)
    };
    if count > 0 && count <= buffer.len() as u32 {
        String::from_utf16_lossy(&buffer[..count as usize - 1])
    } else {
        text.into()
    }
}
fn local_file(text: &str) -> Option<PathBuf> {
    let text = expanded(text.trim().trim_matches('"'));
    if text.contains('\0') || text.starts_with(['\\', '/']) {
        return None;
    }
    let path = PathBuf::from(text);
    // Do not ask the shell to contact remote shares or resolve arbitrary URLs.
    if path.is_absolute() {
        let display = path.to_string_lossy();
        if display.as_bytes().get(1) == Some(&b':')
            && unsafe { GetDriveTypeW(wide(&display[..3]).as_ptr()) } == 4
        {
            return None;
        }
        path.is_file().then_some(path)
    } else {
        let windows = std::env::var_os("SystemRoot").map(PathBuf::from)?;
        [windows.join("System32").join(&path), windows.join(&path)]
            .into_iter()
            .find(|path| path.is_file())
    }
}
fn resolve_command(command: &str) -> Option<PathBuf> {
    let command = expanded(command.trim());
    if let Some(quoted) = command.strip_prefix('"') {
        return local_file(quoted.split('"').next()?);
    }
    // The longest existing executable prefix handles unquoted paths with spaces.
    let mut ends: Vec<usize> = command
        .char_indices()
        .filter_map(|(index, c)| c.is_whitespace().then_some(index))
        .collect();
    ends.push(command.len());
    for end in ends.into_iter().rev() {
        let prefix = &command[..end];
        let extension = Path::new(prefix)
            .extension()
            .and_then(|value| value.to_str());
        if extension.is_some_and(|extension| {
            ["exe", "com", "cmd", "bat", "lnk", "ico", "msc"]
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        }) && let Some(path) = local_file(prefix)
        {
            return Some(path);
        }
    }
    None
}
struct Com(bool);
impl Com {
    fn new() -> Self {
        Self(unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) } >= 0)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn extract(path: &Path, generic: bool) -> Option<Pixels> {
    let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
    let path = wide(&path.to_string_lossy());
    let flags = SHGFI_ICON | SHGFI_LARGEICON | if generic { SHGFI_USEFILEATTRIBUTES } else { 0 };
    let shell_result = unsafe {
        SHGetFileInfoW(
            path.as_ptr(),
            FILE_ATTRIBUTE_NORMAL,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    let result = if shell_result != 0 && !info.hIcon.is_null() {
        let result = raster(info.hIcon);
        unsafe { DestroyIcon(info.hIcon) };
        result
    } else {
        None
    };
    // Executables also have a resource-only path when shell association lookup fails.
    let result = result.or_else(|| {
        if generic {
            return None;
        }
        let mut large = std::ptr::null_mut();
        let mut small = std::ptr::null_mut();
        let count = unsafe { ExtractIconExW(path.as_ptr(), 0, &mut large, &mut small, 1) };
        let icon = if !large.is_null() { large } else { small };
        let rgba = if count > 0 && !icon.is_null() {
            raster(icon)
        } else {
            None
        };
        unsafe {
            if !large.is_null() {
                DestroyIcon(large);
            }
            if !small.is_null() && small != large {
                DestroyIcon(small);
            }
        }
        rgba
    });
    result.map(|rgba| Pixels {
        rgba,
        resolved: !generic,
    })
}
fn raster(icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON) -> Option<Vec<u8>> {
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return None;
        }
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = SIZE as i32;
        info.bmiHeader.biHeight = -(SIZE as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut raw = std::ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut raw, std::ptr::null_mut(), 0);
        if bitmap.is_null() || raw.is_null() {
            DeleteDC(dc);
            return None;
        }
        let previous = SelectObject(dc, bitmap);
        let buffer = std::slice::from_raw_parts_mut(raw.cast::<u32>(), SIZE * SIZE);
        buffer.fill(0);
        let black_ok = DrawIconEx(
            dc,
            0,
            0,
            icon,
            SIZE as i32,
            SIZE as i32,
            0,
            std::ptr::null_mut(),
            DI_NORMAL,
        ) != 0;
        GdiFlush();
        let black = buffer.to_vec();
        buffer.fill(0x00ffffff);
        let white_ok = DrawIconEx(
            dc,
            0,
            0,
            icon,
            SIZE as i32,
            SIZE as i32,
            0,
            std::ptr::null_mut(),
            DI_NORMAL,
        ) != 0;
        GdiFlush();
        let mut rgba = Vec::with_capacity(SIZE * SIZE * 4);
        for (dark, light) in black.iter().zip(buffer.iter()) {
            let dark = dark.to_le_bytes();
            let light = light.to_le_bytes();
            let delta = (0..3)
                .map(|i| light[i].saturating_sub(dark[i]))
                .max()
                .unwrap_or(255);
            let alpha = 255 - delta;
            let straight = |color: u8| {
                if alpha == 0 {
                    0
                } else {
                    (color as u32 * 255 / alpha as u32).min(255) as u8
                }
            };
            rgba.extend_from_slice(&[
                straight(dark[2]),
                straight(dark[1]),
                straight(dark[0]),
                alpha,
            ]);
        }
        SelectObject(dc, previous);
        DeleteObject(bitmap);
        DeleteDC(dc);
        (black_ok && white_ok && rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0))
            .then_some(rgba)
    }
}
fn fallback() -> Pixels {
    let mut rgba = vec![0; SIZE * SIZE * 4];
    for y in 6..26 {
        for x in 5..27 {
            if y < 10 || !(7..25).contains(&x) || y >= 24 {
                let offset = (y * SIZE + x) * 4;
                rgba[offset..offset + 4].copy_from_slice(&[163, 172, 190, 255]);
            }
        }
    }
    Pixels {
        rgba,
        resolved: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_command_resolution_handles_quotes_environment_and_arguments() {
        let root = std::env::var("SystemRoot").unwrap();
        let expected = PathBuf::from(&root).join("System32\\cmd.exe");
        assert_eq!(
            resolve_command("\"%SystemRoot%\\System32\\cmd.exe\" /c exit"),
            Some(expected.clone())
        );
        assert_eq!(resolve_command("cmd.exe /c exit"), Some(expected));
        assert!(resolve_command("https://example.com/program.exe").is_none());
        assert!(resolve_command("\\\\server\\share\\program.exe /quiet").is_none());
    }
    #[test]
    fn native_executable_icon_has_real_color_and_alpha() {
        let _com = Com::new();
        let path = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32\\cmd.exe");
        let icon = extract(&path, false).expect("Windows executable icon");
        assert!(icon.resolved);
        assert_eq!(icon.rgba.len(), SIZE * SIZE * 4);
        assert!(
            icon.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 0)
        );
        assert!(
            icon.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 0)
        );
    }
}
