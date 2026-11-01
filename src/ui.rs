use crate::{appearance::Theme, i18n::Language};
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
mod list;

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
    mutated: bool,
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
                let mutated = !matches!(command, Command::Scan);
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
                        mutated,
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
    icons: crate::icons::ApplicationIcons,
    warnings_open: bool,
    list_metrics: list::ListMetrics,
}
impl Desktop {
    fn construct(ctx: &egui::Context, preview: bool) -> Self {
        let loaded = if preview {
            Ok(Settings::default())
        } else {
            settings::load()
        };
        let message = loaded.as_ref().err().map(|e| {
            (
                true,
                format!(
                    "{}: {e}",
                    Language::English.text("Settings could not be loaded. Defaults are in use.")
                ),
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
            icons: crate::icons::ApplicationIcons::new(ctx.clone(), preview),
            warnings_open: false,
            list_metrics: list::ListMetrics::default(),
        };
        theme(ctx, result.settings.theme);
        load_fonts(ctx);
        if preview {
            result.inventory.entries = model::screen_fixtures();
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
            "selected"=>result.selected=Some("screen-1".into()),
            "many"=>{
                let template=result.inventory.entries.clone();
                result.inventory.entries=(0..500).map(|index|{let mut entry=template[index%template.len()].clone();entry.id=format!("large-{index}");entry.name=format!("{} — {}",entry.name,index+1);entry}).collect();
            },
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
        if matches!(result.page, Page::Startup | Page::Tasks | Page::Services) {
            result.icons.preview(ctx, &result.inventory.entries);
        }
        Ok(result)
    }
    pub fn preview_preferences(
        &mut self,
        ctx: &egui::Context,
        language: Language,
        selected: Theme,
    ) {
        assert!(
            self.preview,
            "Capture preferences require an isolated preview"
        );
        self.settings.language = language;
        self.settings.theme = selected;
        theme(ctx, selected);
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
    pub fn receive_background(&mut self, ctx: &egui::Context) -> bool {
        let mut changed = false;
        if let Some(worker) = &self.worker {
            while let Ok(response) = worker.events.try_recv() {
                self.busy = false;
                changed = true;
                self.message = match response.result {
                    Ok(()) if response.mutated => {
                        Some((false, "Change saved. Undo is available in History.".into()))
                    }
                    Ok(()) => None,
                    Err(error) => Some((
                        true,
                        self.settings
                            .language
                            .format("Operation failed: {error}", &[("error", error.to_string())]),
                    )),
                };
                match response.inventory {
                    Ok(inventory) => {
                        self.icons.request(&inventory.entries);
                        self.inventory = inventory;
                    }
                    Err(error) => {
                        self.message = Some((
                            true,
                            self.settings.language.format(
                                "Operation failed: {error}",
                                &[("error", error.to_string())],
                            ),
                        ))
                    }
                }
                match response.backups {
                    Ok(backups) => self.backups = backups,
                    Err(error) => {
                        self.message = Some((
                            true,
                            self.settings.language.format(
                                "Operation failed: {error}",
                                &[("error", error.to_string())],
                            ),
                        ))
                    }
                }
            }
        }
        changed |= self.icons.receive(ctx);
        let before = update_text(&self.updates, self.settings.language);
        let result = self.updates.poll(self.settings.check_on_startup);
        if update_text(&self.updates, self.settings.language) != before {
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
    pub fn capture_metrics(&self) -> serde_json::Value {
        serde_json::json!({"list":self.list_metrics,"resolved_native_icons":self.icons.resolved_count()})
    }
    pub fn update(&mut self, ctx: &egui::Context) {
        let language = self.settings.language;
        let colors = self.settings.theme.colors();
        egui::TopBottomPanel::top("navigation")
            .frame(
                egui::Frame::new()
                    .fill(colors.panel)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                let previous = self.page;
                ui.horizontal(|ui| {
                    for (page, label) in [
                        (Page::Startup, language.text("Startup apps")),
                        (Page::Tasks, language.text("Scheduled tasks")),
                        (Page::Services, language.text("Services")),
                    ] {
                        ui.selectable_value(&mut self.page, page, label);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.selectable_value(
                            &mut self.page,
                            Page::Settings,
                            language.text("Settings"),
                        );
                        ui.selectable_value(
                            &mut self.page,
                            Page::History,
                            language.text("History"),
                        );
                        if self.updates.update_available()
                            && ui.link(language.text("Update available")).clicked()
                        {
                            self.page = Page::Settings;
                        }
                    });
                });
                if previous != self.page {
                    self.query.clear();
                    self.selected = None;
                }
            });
        self.details_panel(ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(colors.bg)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ctx, |ui| match self.page {
                Page::Settings => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.settings_ui(ui));
                }
                Page::History => self.history_ui(ui),
                _ => self.entries_ui(ui),
            });
        if self.warnings_open {
            egui::Window::new(language.text("Startup locations"))
                .open(&mut self.warnings_open)
                .default_width(540.)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(300.)
                        .show(ui, |ui| {
                            ui.label(language.text("Some all-user startup locations could not be read. Run as administrator to review protected items."));
                            ui.add_space(10.);
                            for warning in &self.inventory.warnings { ui.label(warning); }
                        });
                });
        }
        if let Some((entry, operation)) = self.confirm.clone() {
            egui::Window::new(if operation==Operation::Delete{language.text("Remove startup entry?")}else{language.text("Change system startup?")}).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER,Vec2::ZERO).default_width(430.).show(ctx,|ui|{
            ui.label(RichText::new(&entry.name).strong());ui.add_space(8.);
            ui.label(if operation==Operation::Delete{language.text("A backup is saved first. Removing this entry stops it from starting automatically; it does not uninstall the application.")}else{language.text("Changing a service or system task can affect Windows features, connectivity, or security software. Review its command and purpose before continuing.")});ui.add_space(16.);
            ui.horizontal(|ui|{if ui.button(language.text("Cancel")).clicked(){self.confirm=None;}
                if ui.button(if operation==Operation::Delete{language.text("Back up and remove")}else{language.text("Save backup and continue")}).clicked(){self.confirm=None;self.send(Command::Change(Box::new(entry.clone()),operation));}});
        });
        }
        if self.add_open {
            egui::Window::new(language.text("Add a startup app"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .default_width(460.)
                .show(ctx, |ui| {
                    ui.label(
                        language.text("Start an application when you sign in to this account."),
                    );
                    ui.add_space(12.);
                    ui.label(language.text("Name"));
                    ui.text_edit_singleline(&mut self.add_name);
                    ui.label(language.text("Full executable path"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.add_path)
                            .desired_width(f32::INFINITY)
                            .hint_text("C:\\Program Files\\App\\App.exe"),
                    );
                    if ui
                        .add_enabled(!self.preview, egui::Button::new(language.text("Browse…")))
                        .clicked()
                    {
                        match browse_executable(language) {
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
                            Err(error) => {
                                self.message = Some((
                                    true,
                                    self.settings.language.format(
                                        "Operation failed: {error}",
                                        &[("error", error.to_string())],
                                    ),
                                ))
                            }
                        }
                    }
                    ui.add_space(12.);
                    ui.horizontal(|ui| {
                        if ui.button(language.text("Cancel")).clicked() {
                            self.add_open = false;
                        }
                        if ui.button(language.text("Add app")).clicked() {
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
        let colors = if ui.visuals().dark_mode {
            Theme::Dark
        } else {
            Theme::Light
        }
        .colors();
        ui.label(RichText::new(title).size(19.).strong());
        ui.label(RichText::new(subtitle).size(12.).color(colors.muted));
        ui.add_space(10.);
    }
    fn history_ui(&mut self, ui: &mut egui::Ui) {
        let language = self.settings.language;
        let colors = self.settings.theme.colors();
        Self::heading(
            ui,
            language.text("History & recovery"),
            language.text("Every change keeps its original data. Restore when you need it."),
        );
        ui.label(
            RichText::new(language.text(
                "Backups stay on this computer. Restore never replaces an existing startup entry.",
            ))
            .color(colors.muted),
        );
        ui.add_space(20.);
        self.message_ui(ui);
        let backups = self.backups.clone();
        egui::ScrollArea::vertical().show(ui,|ui|{
            if backups.is_empty(){ui.add_space(40.);ui.label(RichText::new(language.text("No changes yet")).size(20.));ui.label(language.text("Your backups will appear here after you change a startup item."));}
            for backup in backups{egui::Frame::new().fill(colors.panel).corner_radius(8).inner_margin(16).show(ui,|ui|{ui.horizontal(|ui|{ui.vertical(|ui|{ui.label(RichText::new(&backup.entry.name).strong());ui.label(RichText::new(format!("{}  ·  {}  ·  {}",language.text(match backup.operation { Operation::Add => "Add", Operation::Disable => "Disable", Operation::Enable => "Enable", Operation::Delete => "Delete" }),language.text(backup.entry.source_name()),format_time(backup.timestamp))).size(12.).color(colors.muted));ui.label(RichText::new(match &backup.status{Status::Prepared=>language.text("Interrupted change — inspect the original location before recovery").into(),Status::Applied=>language.text("Backup available").into(),Status::Restored=>language.text("Restored").into(),Status::Failed(e)=>language.format("Action failed: {error}", &[("error",e.clone())])}).size(12.).color(if matches!(backup.status,Status::Failed(_)|Status::Prepared){colors.danger}else{colors.muted}));});ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if ui.add_enabled(!self.busy&&backup.status==Status::Applied,egui::Button::new(language.text("Restore"))).clicked(){self.send(Command::Restore(backup.id.clone()));}});});});ui.add_space(10.);}
        });
    }
    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let language = self.settings.language;
        let colors = self.settings.theme.colors();
        Self::heading(
            ui,
            language.text("Settings"),
            language.text("A small utility, with updates on your terms."),
        );
        ui.label(
            RichText::new(language.text("Appearance"))
                .size(19.)
                .strong(),
        );
        let previous = (self.settings.language, self.settings.theme);
        egui::Grid::new("appearance-preferences")
            .spacing([20., 10.])
            .show(ui, |ui| {
                ui.label(language.text("Language"));
                egui::ComboBox::from_id_salt("language")
                    .selected_text(self.settings.language.name())
                    .show_ui(ui, |ui| {
                        for option in Language::ALL {
                            ui.selectable_value(&mut self.settings.language, option, option.name());
                        }
                    });
                ui.end_row();
                ui.label(language.text("Theme"));
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.settings.theme,
                        Theme::Light,
                        language.text("Light"),
                    );
                    ui.selectable_value(
                        &mut self.settings.theme,
                        Theme::Dark,
                        language.text("Dark"),
                    );
                });
                ui.end_row();
            });
        ui.label(
            RichText::new(language.text("Changes apply immediately and are saved automatically."))
                .size(12.)
                .color(colors.muted),
        );
        if previous != (self.settings.language, self.settings.theme) {
            theme(ui.ctx(), self.settings.theme);
            ui.ctx().request_repaint();
            if !self.preview
                && let Err(error) = settings::save(&self.settings)
            {
                self.message = Some((
                    true,
                    self.settings.language.format(
                        "Settings could not be saved: {error}",
                        &[("error", error.to_string())],
                    ),
                ));
            }
        }
        ui.add_space(24.);
        egui::Frame::new()
            .fill(colors.panel)
            .stroke(Stroke::new(1., colors.border))
            .corner_radius(10)
            .inner_margin(24)
            .show(ui, |ui| {
                ui.label(RichText::new(language.text("Updates")).size(20.).strong());
                ui.add_space(8.);
                ui.label(language.format(
                    "Startup Manager  ·  Installed version {version}",
                    &[("version", crate::VERSION.into())],
                ));
                ui.add_space(18.);
                if ui
                    .checkbox(
                        &mut self.settings.check_on_startup,
                        language.text("Check for updates on startup"),
                    )
                    .changed()
                    && !self.preview
                    && let Err(e) = settings::save(&self.settings)
                {
                    self.message = Some((
                        true,
                        language.format(
                            "Settings could not be saved: {error}",
                            &[("error", e.to_string())],
                        ),
                    ));
                }
                ui.label(
                    RichText::new(language.text(
                        "Checks run in the background. Downloads start only when you choose.",
                    ))
                    .size(12.)
                    .color(colors.muted),
                );
                ui.add_space(20.);
                ui.label(RichText::new(update_text(&self.updates, language)).color(
                    if matches!(self.updates.state(), State::Error(_)) {
                        colors.danger
                    } else {
                        colors.text
                    },
                ));
                ui.add_space(12.);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.updates.enabled() && !self.updates.state().busy(),
                            egui::Button::new(language.text("Check for updates")),
                        )
                        .clicked()
                    {
                        self.updates.check();
                    }
                    if self.updates.update_available()
                        && ui.button(language.text("Download and install")).clicked()
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
        ui.label(
            RichText::new(language.text("Backups & data"))
                .size(19.)
                .strong(),
        );
        ui.add_space(8.);
        ui.label(language.text(
            "Settings and recovery backups are stored in your local application data folder.",
        ));
        let data_path = if self.preview {
            PathBuf::from("C:\\Users\\Alex\\AppData\\Local\\StartupManager")
        } else {
            settings::directory()
        };
        ui.label(
            RichText::new(data_path.to_string_lossy())
                .size(12.)
                .monospace()
                .color(colors.muted),
        );
        ui.add_space(12.);
        if ui.button(language.text("Export startup report")).clicked() {
            if self.preview {
                self.export_message =
                    Some(language.text("Export is disabled in demo mode.").into());
            } else {
                let path = settings::directory()
                    .join(format!("startup-report-{}.json", rust_auto_updator::now()));
                self.export_message = Some(match settings::write_json(&path, &self.inventory) {
                    Ok(()) => {
                        language.format("Saved to {path}", &[("path", path.display().to_string())])
                    }
                    Err(e) => {
                        language.format("Export failed: {error}", &[("error", e.to_string())])
                    }
                });
            }
        }
        if let Some(text) = &self.export_message {
            ui.label(
                RichText::new(language.text(text))
                    .size(12.)
                    .color(colors.muted),
            );
        }
        ui.add_space(24.);
        ui.label(RichText::new(language.text("Open source · MIT License")).color(colors.muted));
        ui.hyperlink_to(
            language.text("View project on GitHub"),
            "https://github.com/siarheisinelnikau/startup-manager",
        );
        self.message_ui(ui);
    }
}
fn update_text(updates: &Controller, language: Language) -> String {
    if let Some(reason) = updates.disabled_reason() {
        return language.text(reason).into();
    }
    match updates.state() {
        State::Idle => language.text("Ready to check for a newer version.").into(),
        State::Checking => language.text("Checking GitHub releases…").into(),
        State::Current => language.text("You are using the latest version.").into(),
        State::Available(release) => language.format(
            "Version {version} is available.",
            &[("version", release.version.to_string())],
        ),
        State::Downloading { received, total } => language.format(
            "Downloading: {received} / {total} MiB",
            &[
                ("received", format!("{:.1}", *received as f64 / 1048576.)),
                ("total", format!("{:.1}", *total as f64 / 1048576.)),
            ],
        ),
        State::Installing => language.text("Preparing to restart and install…").into(),
        State::Error(error) => language.format(
            "Update check or installation failed: {error}",
            &[("error", error.clone())],
        ),
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
fn theme(ctx: &egui::Context, selected: Theme) {
    let colors = selected.colors();
    let mut style = egui::Style::default();
    let mut visuals = match selected {
        Theme::Dark => egui::Visuals::dark(),
        Theme::Light => egui::Visuals::light(),
    };
    visuals.panel_fill = colors.bg;
    visuals.window_fill = colors.panel;
    visuals.extreme_bg_color = colors.bg;
    visuals.faint_bg_color = colors.surface;
    visuals.override_text_color = Some(colors.text);
    visuals.selection.bg_fill = colors.surface;
    visuals.selection.stroke = Stroke::new(1., colors.accent);
    visuals.widgets.inactive.bg_fill = colors.surface;
    visuals.widgets.inactive.weak_bg_fill = colors.surface;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1., colors.border);
    visuals.widgets.hovered.bg_fill = colors.hovered;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1., colors.accent);
    visuals.widgets.active.bg_fill = colors.active;
    style.visuals = visuals;
    style.animation_time = 0.;
    style.spacing.item_spacing = Vec2::new(8., 6.);
    style.spacing.button_padding = Vec2::new(10., 6.);
    style.spacing.interact_size.y = 30.;
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(13.));
    ctx.set_style(style);
}
fn load_fonts(ctx: &egui::Context) {
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

fn browse_executable(language: Language) -> Result<Option<PathBuf>> {
    use windows_sys::Win32::UI::{Controls::Dialogs::*, Input::KeyboardAndMouse::GetActiveWindow};
    let mut filename = vec![0u16; 32768];
    let filter: Vec<u16> = format!("{}\0*.exe\0\0", language.text("Applications (*.exe)"))
        .encode_utf16()
        .collect();
    let title: Vec<u16> = format!("{}\0", language.text("Choose a startup application"))
        .encode_utf16()
        .collect();
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
    fn laid_out(screen: &str, size: egui::Vec2) -> Desktop {
        let ctx = egui::Context::default();
        let mut desktop = Desktop::preview(&ctx, screen).unwrap();
        for frame in 0..4 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    time: Some(frame as f64 / 60.),
                    ..Default::default()
                },
                |ctx| desktop.update(ctx),
            );
        }
        desktop
    }
    #[test]
    fn thirteen_startup_entries_fit_without_sidebar_or_empty_details_reservation() {
        for size in [egui::vec2(1120., 760.), egui::vec2(900., 620.)] {
            let desktop = laid_out("startup", size);
            let metrics = desktop.capture_metrics();
            assert_eq!(metrics["list"]["total_rows"], 13);
            assert!(
                metrics["list"]["first_name_left"].as_f64().unwrap() <= 64.,
                "Names must remain beside their icons: {metrics}"
            );
            assert_eq!(
                metrics["list"]["fully_visible_rows"], 13,
                "{size:?}: {metrics}"
            );
        }
        let selected = laid_out("selected", egui::vec2(1120., 760.));
        assert_eq!(selected.capture_metrics()["list"]["fully_visible_rows"], 13);
    }
    #[test]
    fn every_language_and_theme_preserves_compact_list_and_settings_layout() {
        for language in Language::ALL {
            for theme in [Theme::Dark, Theme::Light] {
                for screen in ["startup", "settings", "history", "error", "selected"] {
                    let ctx = egui::Context::default();
                    let mut desktop = Desktop::preview(&ctx, screen).unwrap();
                    desktop.preview_preferences(&ctx, language, theme);
                    for frame in 0..4 {
                        let _ = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(900., 620.),
                                )),
                                time: Some(frame as f64 / 60.),
                                ..Default::default()
                            },
                            |ctx| desktop.update(ctx),
                        );
                    }
                    assert_eq!(ctx.style().visuals.dark_mode, theme == Theme::Dark);
                    if screen == "startup" {
                        assert_eq!(
                            desktop.capture_metrics()["list"]["fully_visible_rows"],
                            13,
                            "{} {theme:?}",
                            language.name()
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn large_inventory_only_lays_out_visible_rows() {
        let desktop = laid_out("many", egui::vec2(1120., 760.));
        let metrics = desktop.capture_metrics();
        assert_eq!(metrics["list"]["total_rows"], 500);
        let visible = metrics["list"]["fully_visible_rows"].as_u64().unwrap();
        assert!((13..30).contains(&visible), "{metrics}");
    }
    #[test]
    fn row_selection_and_inline_action_use_the_real_widgets() {
        let ctx = egui::Context::default();
        let mut desktop = Desktop::preview(&ctx, "startup").unwrap();
        let mut frame = 0;
        let mut run = |desktop: &mut Desktop, events: Vec<egui::Event>| {
            frame += 1;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1120., 760.),
                    )),
                    time: Some(frame as f64 / 60.),
                    events,
                    ..Default::default()
                },
                |ctx| desktop.update(ctx),
            );
        };
        for _ in 0..3 {
            run(&mut desktop, vec![]);
        }
        let point = egui::pos2(80., 142.);
        run(
            &mut desktop,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        run(
            &mut desktop,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(desktop.selected.as_deref(), Some("screen-0"));
        run(&mut desktop, vec![]);
        let point = egui::pos2(1056., 142.);
        run(
            &mut desktop,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        run(
            &mut desktop,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(
            desktop
                .message
                .as_ref()
                .is_some_and(|(_, text)| text.starts_with("Demo mode:")),
            "The inline action must reach the isolated command dispatcher"
        );
    }
}
