use crate::{
    engine::Engine,
    model::{self, Backup, Entry, Inventory, Operation, Source, Status},
    settings::{self, Settings},
    updater,
};
use anyhow::Result;
use egui::{Color32, RichText, Stroke, Vec2};
use rust_auto_updator::{Controller, State};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};

const BG: Color32 = Color32::from_rgb(17, 23, 31);
const PANEL: Color32 = Color32::from_rgb(23, 31, 41);
const SURFACE: Color32 = Color32::from_rgb(30, 40, 52);
const TEXT: Color32 = Color32::from_rgb(232, 239, 244);
const MUTED: Color32 = Color32::from_rgb(157, 174, 191);
const ACCENT: Color32 = Color32::from_rgb(88, 218, 187);
const BORDER: Color32 = Color32::from_rgb(45, 59, 74);
const DANGER: Color32 = Color32::from_rgb(255, 151, 151);

enum Command {
    Scan,
    Change(Box<Entry>, Operation),
    Restore(String),
    Add(String, PathBuf),
}
struct Response {
    result: Result<()>,
    inventory: Result<Inventory>,
    backups: Result<Vec<Backup>>,
}
struct Worker {
    commands: Sender<Command>,
    events: Receiver<Response>,
}
impl Worker {
    fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            let engine = Engine::new();
            while let Ok(command) = rx.recv() {
                let result = match command {
                    Command::Scan => Ok(()),
                    Command::Change(entry, operation) => engine.apply(&entry, operation),
                    Command::Restore(id) => engine.restore(&id),
                    Command::Add(name, path) => engine.add(&name, &path),
                };
                let inventory = crate::windows_sources::scan().and_then(|mut report| {
                    engine.merge_disabled(&mut report)?;
                    Ok(report)
                });
                let backups = engine.backups();
                if events_tx
                    .send(Response {
                        result,
                        inventory,
                        backups,
                    })
                    .is_err()
                {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self {
            commands: tx,
            events,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Startup,
    Tasks,
    Services,
    History,
    Settings,
}
pub struct Desktop {
    page: Page,
    query: String,
    filter: u8,
    inventory: Inventory,
    backups: Vec<Backup>,
    selected: Option<String>,
    settings: Settings,
    updates: Controller,
    worker: Option<Worker>,
    busy: bool,
    message: Option<(bool, String)>,
    preview: bool,
    exit: bool,
    closing: bool,
    pending_install: bool,
    confirm: Option<(Entry, Operation)>,
    add_open: bool,
    add_name: String,
    add_path: String,
    export_message: Option<String>,
}
impl Desktop {
    fn construct(ctx: &egui::Context, preview: bool) -> Self {
        theme(ctx);
        let loaded = if preview {
            Ok(Settings::default())
        } else {
            settings::load()
        };
        let message = loaded.as_ref().err().map(|e| {
            (
                true,
                format!("Settings could not be loaded: {e}. Defaults are in use."),
            )
        });
        let worker = if preview {
            None
        } else {
            Some(Worker::new(ctx.clone()))
        };
        let mut result = Self {
            page: Page::Startup,
            query: String::new(),
            filter: 0,
            inventory: Inventory::default(),
            backups: vec![],
            selected: None,
            settings: loaded.unwrap_or_default(),
            updates: updater::start(ctx.clone(), preview),
            worker,
            busy: false,
            message,
            preview,
            exit: false,
            closing: false,
            pending_install: false,
            confirm: None,
            add_open: false,
            add_name: String::new(),
            add_path: String::new(),
            export_message: None,
        };
        if preview {
            result.inventory.entries = model::fixtures();
            result.selected = Some("fixture-1".into());
        } else {
            result.send(Command::Scan);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(6));
                ctx.request_repaint();
            });
        }
        result
    }
    pub fn new(ctx: &egui::Context) -> Self {
        Self::construct(ctx, false)
    }
    pub fn preview(ctx: &egui::Context, screen: &str) -> Result<Self> {
        let mut result = Self::construct(ctx, true);
        match screen {
            "settings" => result.page=Page::Settings,
            "empty" => {result.inventory.entries.clear();result.selected=None;},
            "error" => result.message=Some((true,"Some all-user startup locations could not be read. Run as administrator to review protected items.".into())),
            "history" => {
                result.page=Page::History;
                result.backups.push(Backup {
                    id:"preview".into(),timestamp:1793500000,operation:Operation::Disable,
                    entry:model::fixtures()[2].clone(),status:Status::Applied,post_source:None,
                });
            }
            _ => {}
        }
        Ok(result)
    }
    pub fn use_live_preview_clock(&mut self) {}
    pub fn diagnostic_settings(&mut self) {
        if self.preview {
            self.page = Page::Settings;
        }
    }
    pub fn exit_requested(&self) -> bool {
        self.exit
    }
    pub fn request_close(&mut self) -> bool {
        if self.busy {
            self.closing = true;
            self.message = Some((
                false,
                "Finishing the current operation before closing…".into(),
            ));
            false
        } else {
            true
        }
    }
    fn send(&mut self, command: Command) {
        if self.closing || self.pending_install {
            return;
        }
        if self.preview {
            self.message = Some((
                false,
                "Demo mode: startup entries are examples and actions are disabled.".into(),
            ));
            return;
        }
        if self.busy {
            return;
        }
        match self
            .worker
            .as_ref()
            .map(|worker| worker.commands.send(command))
        {
            Some(Ok(())) => {
                self.busy = true;
                self.message = None;
            }
            _ => {
                self.message = Some((
                    true,
                    "The worker stopped. Reopen the application to continue.".into(),
                ))
            }
        }
    }
    pub fn receive_background(&mut self, _ctx: &egui::Context) -> bool {
        let mut changed = false;
        if let Some(worker) = &self.worker {
            while let Ok(response) = worker.events.try_recv() {
                self.busy = false;
                changed = true;
                self.message = Some(match response.result {
                    Ok(()) => (
                        false,
                        "Startup list refreshed. Changes apply at the next sign-in or restart."
                            .into(),
                    ),
                    Err(error) => (true, error.to_string()),
                });
                match response.inventory {
                    Ok(inventory) => self.inventory = inventory,
                    Err(error) => self.message = Some((true, error.to_string())),
                }
                match response.backups {
                    Ok(backups) => self.backups = backups,
                    Err(error) => self.message = Some((true, error.to_string())),
                }
            }
        }
        let before = update_text(&self.updates);
        let result = self.updates.poll(self.settings.check_on_startup);
        if update_text(&self.updates) != before {
            changed = true;
        }
        if result.ready_to_install {
            self.pending_install = true;
        }
        if self.pending_install && !self.busy {
            self.pending_install = false;
            match settings::save(&self.settings).and_then(|_| updater::install(&mut self.updates)) {
                Ok(()) => self.exit = true,
                Err(error) => self.updates.installation_failed(error.to_string()),
            }
        }
        if self.closing && !self.busy {
            self.exit = true;
        }
        changed
    }
    pub fn update(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation")
            .exact_width(190.)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(22))
            .show(ctx, |ui| {
                ui.add_space(8.);
                ui.label(RichText::new("↗").size(32.).color(ACCENT));
                ui.add_space(8.);
                ui.label(RichText::new("Startup\nManager").size(23.).strong());
                ui.add_space(8.);
                ui.label(RichText::new("A lighter sign-in.").color(MUTED));
                ui.add_space(38.);
                for (page, label) in [
                    (Page::Startup, "Startup apps"),
                    (Page::Tasks, "Scheduled tasks"),
                    (Page::Services, "Services"),
                    (Page::History, "History"),
                    (Page::Settings, "Settings"),
                ] {
                    let selected = self.page == page;
                    let button = egui::Button::new(RichText::new(label).color(if selected {
                        ACCENT
                    } else {
                        TEXT
                    }))
                    .fill(if selected {
                        SURFACE
                    } else {
                        Color32::TRANSPARENT
                    })
                    .min_size(Vec2::new(146., 42.));
                    if ui.add(button).clicked() {
                        self.page = page;
                        self.query.clear();
                        self.selected = None;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new(format!("v{}  ·  MIT", crate::VERSION))
                            .size(12.)
                            .color(MUTED),
                    );
                    if self.preview {
                        ui.label(RichText::new("DEMO · NO CHANGES").size(11.).color(ACCENT));
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(30))
            .show(ctx, |ui| match self.page {
                Page::Settings => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.settings_ui(ui));
                }
                Page::History => self.history_ui(ui),
                _ => self.entries_ui(ui),
            });
        if let Some((entry, operation)) = self.confirm.clone() {
            egui::Window::new(if operation==Operation::Delete{"Remove startup entry?"}else{"Change system startup?"}).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER,Vec2::ZERO).default_width(430.).show(ctx,|ui|{
            ui.label(RichText::new(&entry.name).strong());ui.add_space(8.);
            ui.label(if operation==Operation::Delete{"A backup is saved first. Removing this entry stops it from starting automatically; it does not uninstall the application."}else{"Changing a service or system task can affect Windows features, connectivity, or security software. Review its command and purpose before continuing."});ui.add_space(16.);
            ui.horizontal(|ui|{if ui.button("Cancel").clicked(){self.confirm=None;}
                if ui.button(if operation==Operation::Delete{"Back up and remove"}else{"Save backup and continue"}).clicked(){self.confirm=None;self.send(Command::Change(Box::new(entry.clone()),operation));}});
        });
        }
        if self.add_open {
            egui::Window::new("Add a startup app")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .default_width(460.)
                .show(ctx, |ui| {
                    ui.label("Start an application when you sign in to this account.");
                    ui.add_space(12.);
                    ui.label("Name");
                    ui.text_edit_singleline(&mut self.add_name);
                    ui.label("Full executable path");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.add_path)
                            .desired_width(f32::INFINITY)
                            .hint_text("C:\\Program Files\\App\\App.exe"),
                    );
                    if ui
                        .add_enabled(!self.preview, egui::Button::new("Browse…"))
                        .clicked()
                    {
                        match browse_executable() {
                            Ok(Some(path)) => {
                                self.add_path = path.to_string_lossy().into();
                                if self.add_name.trim().is_empty() {
                                    self.add_name = path
                                        .file_stem()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into();
                                }
                            }
                            Ok(None) => {}
                            Err(error) => self.message = Some((true, error.to_string())),
                        }
                    }
                    ui.add_space(12.);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.add_open = false;
                        }
                        if ui.button("Add app").clicked() {
                            self.add_open = false;
                            self.send(Command::Add(
                                self.add_name.clone(),
                                PathBuf::from(self.add_path.trim()),
                            ));
                        }
                    });
                });
        }
        if self.exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
        ui.label(RichText::new(title).size(29.).strong());
        ui.add_space(6.);
        ui.label(RichText::new(subtitle).color(MUTED));
        ui.add_space(24.);
    }
    fn entries_ui(&mut self, ui: &mut egui::Ui) {
        let (title, subtitle, category) = match self.page {
            Page::Tasks => (
                "Scheduled tasks",
                "Review tasks triggered at sign-in or system startup.",
                "Scheduled tasks",
            ),
            Page::Services => (
                "Services",
                "Review automatic and disabled services. Changes do not stop running services.",
                "Services",
            ),
            _ => (
                "Take control of startup.",
                "Choose what starts with Windows. Keep what you need.",
                "Startup apps",
            ),
        };
        Self::heading(ui, title, subtitle);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Search name, command or location…")
                    .desired_width((ui.available_width() - 345.).max(140.)),
            );
            egui::ComboBox::from_id_salt("state")
                .selected_text(["All states", "Enabled", "Disabled"][self.filter as usize])
                .width(100.)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.filter, 0, "All states");
                    ui.selectable_value(&mut self.filter, 1, "Enabled");
                    ui.selectable_value(&mut self.filter, 2, "Disabled");
                });
            if ui
                .add_enabled(!self.busy, egui::Button::new("Refresh"))
                .clicked()
            {
                self.send(Command::Scan);
            }
            if self.page == Page::Startup
                && ui
                    .add_enabled(!self.busy, egui::Button::new("+ Add app"))
                    .clicked()
            {
                self.add_open = true;
            }
        });
        ui.add_space(14.);
        let entries: Vec<Entry> = self
            .inventory
            .entries
            .iter()
            .filter(|e| {
                e.category() == category
                    && e.matches(&self.query)
                    && (self.filter == 0 || e.enabled == (self.filter == 1))
            })
            .cloned()
            .collect();
        let enabled = entries.iter().filter(|e| e.enabled).count();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} items", entries.len())).strong());
            ui.label(RichText::new(format!("·  {enabled} enabled")).color(MUTED));
            if self.busy {
                ui.label(RichText::new("Reading Windows startup items…").color(ACCENT));
            }
        });
        self.message_ui(ui);
        self.warnings_ui(ui);
        ui.add_space(10.);
        let list_height = (ui.available_height() - 205.).max(30.);
        egui::Frame::new()
            .fill(PANEL)
            .stroke(Stroke::new(1., BORDER))
            .corner_radius(10)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(10.);
                    ui.label(RichText::new("APPLICATION / SOURCE").size(11.).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(15.);
                        ui.label(RichText::new("STATE").size(11.).color(MUTED));
                    });
                });
                ui.add_space(8.);
                egui::ScrollArea::vertical()
                    .max_height(list_height)
                    .min_scrolled_height(list_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if entries.is_empty() {
                            ui.add_space(35.);
                            ui.label(
                                RichText::new(if self.busy {
                                    "Reading startup locations…"
                                } else {
                                    "No matching startup items"
                                })
                                .size(19.),
                            );
                            ui.label(
                                RichText::new("Try another search or refresh the list.")
                                    .color(MUTED),
                            );
                        }
                        for entry in &entries {
                            let selected = self.selected.as_ref() == Some(&entry.id);
                            let frame = egui::Frame::new()
                                .fill(if selected {
                                    SURFACE
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .corner_radius(7)
                                .inner_margin(10)
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        let (rect, _) = ui.allocate_exact_size(
                                            Vec2::splat(34.),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(
                                            rect,
                                            7.,
                                            Color32::from_rgb(39, 68, 68),
                                        );
                                        ui.painter().text(
                                            rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            entry.name.chars().next().unwrap_or('?'),
                                            egui::FontId::proportional(17.),
                                            ACCENT,
                                        );
                                        ui.add_space(8.);
                                        ui.vertical(|ui| {
                                            ui.label(RichText::new(&entry.name).strong().size(15.));
                                            ui.label(
                                                RichText::new(format!(
                                                    "{}  ·  {}",
                                                    entry.source_name(),
                                                    entry.scope
                                                ))
                                                .size(12.)
                                                .color(MUTED),
                                            );
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(if entry.enabled {
                                                        "●  Enabled"
                                                    } else {
                                                        "○  Disabled"
                                                    })
                                                    .size(12.)
                                                    .color(if entry.enabled {
                                                        ACCENT
                                                    } else {
                                                        MUTED
                                                    }),
                                                );
                                            },
                                        );
                                    });
                                });
                            let response = ui.interact(
                                frame.response.rect,
                                ui.id().with(&entry.id),
                                egui::Sense::click(),
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::SelectableLabel,
                                    true,
                                    selected,
                                    &entry.name,
                                )
                            });
                            if response.has_focus() {
                                ui.painter().rect_stroke(
                                    frame.response.rect,
                                    7.,
                                    Stroke::new(1., ACCENT),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            if response.clicked() {
                                self.selected = Some(entry.id.clone());
                            }
                            ui.add_space(2.);
                        }
                    });
            });
        ui.add_space(14.);
        if let Some(entry) = entries
            .iter()
            .find(|e| Some(&e.id) == self.selected.as_ref())
            .cloned()
        {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&entry.name).size(17.).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !self.busy
                                && !matches!(entry.source, Source::Service { .. })
                                && entry.backup_id.is_none(),
                            egui::Button::new(RichText::new("Remove…").color(DANGER)),
                        )
                        .clicked()
                    {
                        self.confirm = Some((entry.clone(), Operation::Delete));
                    }
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new(if entry.enabled { "Disable" } else { "Enable" })
                                .fill(Color32::from_rgb(36, 86, 77)),
                        )
                        .clicked()
                    {
                        let operation = if entry.enabled {
                            Operation::Disable
                        } else {
                            Operation::Enable
                        };
                        if matches!(entry.source, Source::Task { .. } | Source::Service { .. }) {
                            self.confirm = Some((entry.clone(), operation));
                        } else {
                            self.send(Command::Change(Box::new(entry.clone()), operation));
                        }
                    }
                });
            });
            ui.add_space(8.);
            ui.add(
                egui::Label::new(
                    RichText::new(&entry.command)
                        .monospace()
                        .size(12.)
                        .color(MUTED),
                )
                .truncate(),
            )
            .on_hover_text(&entry.command);
            ui.add(
                egui::Label::new(RichText::new(&entry.location).size(12.).color(MUTED)).truncate(),
            )
            .on_hover_text(&entry.location);
            if ui.small_button("Copy details").clicked() {
                ui.ctx().copy_text(format!(
                    "{}\n{}\n{}",
                    entry.name, entry.command, entry.location
                ));
            }
        } else {
            ui.label(
                RichText::new("Select an item to review its command and change startup behavior.")
                    .color(MUTED),
            );
        }
        if self.updates.update_available() {
            ui.add_space(8.);
            if ui.link("An update is available · Open Settings").clicked() {
                self.page = Page::Settings;
            }
        }
    }
    fn warnings_ui(&self, ui: &mut egui::Ui) {
        if !self.inventory.warnings.is_empty() {
            ui.add_space(6.);
            egui::CollapsingHeader::new(format!(
                "{} locations or items need attention",
                self.inventory.warnings.len()
            ))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("inventory-warnings")
                    .max_height(48.)
                    .show(ui, |ui| {
                        for warning in &self.inventory.warnings {
                            ui.colored_label(DANGER, warning);
                        }
                    });
            });
        }
    }
    fn message_ui(&self, ui: &mut egui::Ui) {
        if let Some((error, text)) = &self.message {
            ui.add_space(10.);
            egui::ScrollArea::vertical()
                .id_salt("status-message")
                .max_height(64.)
                .show(ui, |ui| {
                    ui.label(RichText::new(text).size(12.).color(if *error {
                        DANGER
                    } else {
                        MUTED
                    }));
                });
        }
    }
    fn history_ui(&mut self, ui: &mut egui::Ui) {
        Self::heading(
            ui,
            "History & recovery",
            "Every change keeps its original data. Restore when you need it.",
        );
        ui.label(
            RichText::new(
                "Backups stay on this computer. Restore never replaces an existing startup entry.",
            )
            .color(MUTED),
        );
        ui.add_space(20.);
        self.message_ui(ui);
        let backups = self.backups.clone();
        egui::ScrollArea::vertical().show(ui,|ui|{
            if backups.is_empty(){ui.add_space(40.);ui.label(RichText::new("No changes yet").size(20.));ui.label("Your backups will appear here after you change a startup item.");}
            for backup in backups{egui::Frame::new().fill(PANEL).corner_radius(8).inner_margin(16).show(ui,|ui|{ui.horizontal(|ui|{ui.vertical(|ui|{ui.label(RichText::new(&backup.entry.name).strong());ui.label(RichText::new(format!("{:?}  ·  {}  ·  {}",backup.operation,backup.entry.source_name(),format_time(backup.timestamp))).size(12.).color(MUTED));ui.label(RichText::new(match &backup.status{Status::Prepared=>"Interrupted change — inspect the original location before recovery".into(),Status::Applied=>"Backup available".into(),Status::Restored=>"Restored".into(),Status::Failed(e)=>format!("Action failed: {e}")}).size(12.).color(if matches!(backup.status,Status::Failed(_)|Status::Prepared){DANGER}else{MUTED}));});ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if ui.add_enabled(!self.busy&&backup.status==Status::Applied,egui::Button::new("Restore")).clicked(){self.send(Command::Restore(backup.id.clone()));}});});});ui.add_space(10.);}
        });
    }
    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        Self::heading(
            ui,
            "Settings",
            "A small utility, with updates on your terms.",
        );
        egui::Frame::new()
            .fill(PANEL)
            .stroke(Stroke::new(1., BORDER))
            .corner_radius(10)
            .inner_margin(24)
            .show(ui, |ui| {
                ui.label(RichText::new("Updates").size(20.).strong());
                ui.add_space(8.);
                ui.label(format!(
                    "Startup Manager  ·  Installed version {}",
                    crate::VERSION
                ));
                ui.add_space(18.);
                if ui
                    .checkbox(
                        &mut self.settings.check_on_startup,
                        "Check for updates on startup",
                    )
                    .changed()
                    && !self.preview
                    && let Err(e) = settings::save(&self.settings)
                {
                    self.message = Some((true, e.to_string()));
                }
                ui.label(
                    RichText::new(
                        "Checks run in the background. Downloads start only when you choose.",
                    )
                    .size(12.)
                    .color(MUTED),
                );
                ui.add_space(20.);
                ui.label(RichText::new(update_text(&self.updates)).color(
                    if matches!(self.updates.state(), State::Error(_)) {
                        DANGER
                    } else {
                        TEXT
                    },
                ));
                ui.add_space(12.);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.updates.enabled() && !self.updates.state().busy(),
                            egui::Button::new("Check for updates"),
                        )
                        .clicked()
                    {
                        self.updates.check();
                    }
                    if self.updates.update_available()
                        && ui.button("Download and install").clicked()
                    {
                        self.updates.download_available();
                    }
                });
                if let State::Downloading { received, total } = self.updates.state() {
                    let ratio = if *total > 0 {
                        *received as f32 / *total as f32
                    } else {
                        0.
                    };
                    ui.add(egui::ProgressBar::new(ratio).show_percentage());
                }
                if let State::Available(release) = self.updates.state() {
                    ui.add_space(12.);
                    egui::ScrollArea::vertical()
                        .max_height(140.)
                        .show(ui, |ui| {
                            ui.label(&release.notes);
                        });
                }
            });
        ui.add_space(24.);
        ui.label(RichText::new("Backups & data").size(19.).strong());
        ui.add_space(8.);
        ui.label("Settings and recovery backups are stored in your local application data folder.");
        let data_path = if self.preview {
            PathBuf::from("C:\\Users\\Alex\\AppData\\Local\\StartupManager")
        } else {
            settings::directory()
        };
        ui.label(
            RichText::new(data_path.to_string_lossy())
                .size(12.)
                .monospace()
                .color(MUTED),
        );
        ui.add_space(12.);
        if ui.button("Export startup report").clicked() {
            if self.preview {
                self.export_message = Some("Export is disabled in demo mode.".into());
            } else {
                let path = settings::directory()
                    .join(format!("startup-report-{}.json", rust_auto_updator::now()));
                self.export_message = Some(match settings::write_json(&path, &self.inventory) {
                    Ok(()) => format!("Saved to {}", path.display()),
                    Err(e) => format!("Export failed: {e}"),
                });
            }
        }
        if let Some(text) = &self.export_message {
            ui.label(RichText::new(text).size(12.).color(MUTED));
        }
        ui.add_space(24.);
        ui.label(RichText::new("Open source · MIT License").color(MUTED));
        ui.hyperlink_to(
            "View project on GitHub",
            "https://github.com/siarheisinelnikau/startup-manager",
        );
        self.message_ui(ui);
    }
}
fn update_text(updates: &Controller) -> String {
    if let Some(reason) = updates.disabled_reason() {
        return reason.into();
    }
    match updates.state() {
        State::Idle => "Ready to check for a newer version.".into(),
        State::Checking => "Checking GitHub releases…".into(),
        State::Current => "You are using the latest version.".into(),
        State::Available(release) => format!("Version {} is available.", release.version),
        State::Downloading { received, total } => format!(
            "Downloading: {:.1} / {:.1} MiB",
            *received as f64 / 1048576.,
            *total as f64 / 1048576.
        ),
        State::Installing => "Preparing to restart and install…".into(),
        State::Error(error) => format!("Update check or installation failed: {error}"),
    }
}
fn format_time(timestamp: u64) -> String {
    let days = (timestamp / 86400) as i64 + 719468;
    let era = days / 146097;
    let day_of_era = days - era * 146097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}  {:02}:{:02} UTC",
        timestamp / 3600 % 24,
        timestamp / 60 % 60
    )
}
fn theme(ctx: &egui::Context) {
    let mut style = egui::Style::default();
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = SURFACE;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = Color32::from_rgb(37, 84, 79);
    visuals.selection.stroke = Stroke::new(1., ACCENT);
    visuals.widgets.inactive.bg_fill = SURFACE;
    visuals.widgets.inactive.weak_bg_fill = SURFACE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1., BORDER);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(43, 62, 76);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1., ACCENT);
    visuals.widgets.active.bg_fill = Color32::from_rgb(42, 81, 76);
    style.visuals = visuals;
    style.animation_time = 0.;
    style.spacing.item_spacing = Vec2::new(10., 8.);
    style.spacing.button_padding = Vec2::new(12., 8.);
    style.spacing.interact_size.y = 34.;
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(13.));
    ctx.set_style(style);
    if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("Segoe UI".into(), egui::FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "Segoe UI".into());
        ctx.set_fonts(fonts);
    }
}

fn browse_executable() -> Result<Option<PathBuf>> {
    use windows_sys::Win32::UI::{Controls::Dialogs::*, Input::KeyboardAndMouse::GetActiveWindow};
    let mut filename = vec![0u16; 32768];
    let filter: Vec<u16> = "Applications (*.exe)\0*.exe\0\0".encode_utf16().collect();
    let title: Vec<u16> = "Choose a startup application\0".encode_utf16().collect();
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.hwndOwner = unsafe { GetActiveWindow() };
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrFile = filename.as_mut_ptr();
    dialog.nMaxFile = filename.len() as u32;
    dialog.lpstrTitle = title.as_ptr();
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_DONTADDTORECENT;
    if unsafe { GetOpenFileNameW(&mut dialog) } != 0 {
        let length = filename
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(filename.len());
        return Ok(Some(PathBuf::from(String::from_utf16(
            &filename[..length],
        )?)));
    }
    let error = unsafe { CommDlgExtendedError() };
    anyhow::ensure!(error == 0, "Cannot open the application picker ({error})");
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readable_utc_history_dates() {
        assert_eq!(format_time(0), "1970-01-01  00:00 UTC");
        assert_eq!(format_time(1793491200), "2026-11-01  00:00 UTC");
    }
}
