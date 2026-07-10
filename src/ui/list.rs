use super::*;

const ROW_HEIGHT: f32 = 36.;
const HEADER_HEIGHT: f32 = 26.;
#[derive(Default, serde::Serialize)]
pub(super) struct ListMetrics {
    total_rows: usize,
    fully_visible_rows: usize,
    list_height: f32,
    selected_details: bool,
    first_name_left: Option<f32>,
}
struct Columns {
    name: egui::Rect,
    source: egui::Rect,
    scope: egui::Rect,
    state: egui::Rect,
    action: egui::Rect,
}
impl Columns {
    fn new(rect: egui::Rect) -> Self {
        let action = egui::Rect::from_min_max(egui::pos2(rect.right() - 78., rect.top()), rect.max);
        let state = egui::Rect::from_min_max(
            egui::pos2(action.left() - 50., rect.top()),
            egui::pos2(action.left() - 8., rect.bottom()),
        );
        let scope = egui::Rect::from_min_max(
            egui::pos2(state.left() - 120., rect.top()),
            egui::pos2(state.left() - 8., rect.bottom()),
        );
        let source = egui::Rect::from_min_max(
            egui::pos2(scope.left() - 122., rect.top()),
            egui::pos2(scope.left() - 8., rect.bottom()),
        );
        let name =
            egui::Rect::from_min_max(rect.min, egui::pos2(source.left() - 12., rect.bottom()));
        Self {
            name,
            source,
            scope,
            state,
            action,
        }
    }
}
fn status_icon(painter: &egui::Painter, rect: egui::Rect, enabled: bool) {
    let center = rect.center();
    let stroke = Stroke::new(1.35, if enabled { TEXT } else { MUTED });
    painter.circle_stroke(center, 6.5, stroke);
    if enabled {
        painter.add(egui::Shape::line(
            vec![
                center + egui::vec2(-3., 0.),
                center + egui::vec2(-0.5, 2.5),
                center + egui::vec2(3.5, -2.),
            ],
            stroke,
        ));
    } else {
        painter.line_segment(
            [center - egui::vec2(3., 0.), center + egui::vec2(3., 0.)],
            stroke,
        );
    }
}
fn text(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    label: impl Into<String>,
    color: Color32,
    strong: bool,
) -> egui::Response {
    let label = RichText::new(label.into()).size(13.).color(color);
    let label = if strong { label.strong() } else { label };
    let mut cell = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    cell.add(
        egui::Label::new(label)
            .truncate()
            .halign(egui::Align::LEFT)
            .sense(egui::Sense::hover()),
    )
}
impl Desktop {
    fn category(&self) -> Option<&'static str> {
        match self.page {
            Page::Startup => Some("Startup apps"),
            Page::Tasks => Some("Scheduled tasks"),
            Page::Services => Some("Services"),
            _ => None,
        }
    }
    fn entry_visible(&self, entry: &Entry) -> bool {
        Some(entry.category()) == self.category()
            && entry.matches(&self.query)
            && (self.filter == 0 || entry.enabled == (self.filter == 1))
    }
    pub(super) fn details_panel(&mut self, ctx: &egui::Context) {
        let entry = self
            .inventory
            .entries
            .iter()
            .find(|entry| self.selected.as_ref() == Some(&entry.id) && self.entry_visible(entry))
            .cloned();
        let Some(entry) = entry else {
            return;
        };
        egui::TopBottomPanel::bottom("entry-details")
            .exact_height(96.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .stroke(Stroke::new(1., BORDER)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 3.;
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 300.).max(100.);
                    let (name_rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, 28.), egui::Sense::hover());
                    text(ui, name_rect, &entry.name, TEXT, true);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Close details").clicked() {
                            self.selected = None;
                        }
                        if ui
                            .add_enabled(
                                !self.busy
                                    && !matches!(entry.source, Source::Service { .. })
                                    && entry.backup_id.is_none(),
                                egui::Button::new(RichText::new("Remove…").color(DANGER)).small(),
                            )
                            .clicked()
                        {
                            self.confirm = Some((entry.clone(), Operation::Delete));
                        }
                        if ui.small_button("Copy details").clicked() {
                            ui.ctx().copy_text(format!(
                                "{}\n{}\n{}",
                                entry.name, entry.command, entry.location
                            ));
                        }
                    });
                });
                ui.add(
                    egui::Label::new(
                        RichText::new(&entry.command)
                            .monospace()
                            .size(12.)
                            .color(TEXT),
                    )
                    .truncate(),
                )
                .on_hover_text(&entry.command);
                ui.add(
                    egui::Label::new(RichText::new(&entry.location).size(12.).color(MUTED))
                        .truncate(),
                )
                .on_hover_text(&entry.location);
            });
    }
    pub(super) fn entries_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let tail = if self.page == Page::Startup {
                292.
            } else {
                200.
            } + if self.inventory.warnings.is_empty() {
                0.
            } else {
                92.
            };
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Search name, command or location")
                    .desired_width((ui.available_width() - tail).max(100.)),
            );
            egui::ComboBox::from_id_salt("state")
                .selected_text(["All states", "Enabled", "Disabled"][self.filter as usize])
                .width(94.)
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
            if !self.inventory.warnings.is_empty()
                && ui
                    .button(format!(
                        "{} notice{}",
                        self.inventory.warnings.len(),
                        if self.inventory.warnings.len() == 1 {
                            ""
                        } else {
                            "s"
                        }
                    ))
                    .clicked()
            {
                self.warnings_open = true;
            }
        });
        self.message_ui(ui);
        ui.add_space(4.);
        let indices: Vec<usize> = self
            .inventory
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| self.entry_visible(entry).then_some(index))
            .collect();
        let enabled = indices
            .iter()
            .filter(|index| self.inventory.entries[**index].enabled)
            .count();
        let scroll_height = (ui.available_height() - HEADER_HEIGHT).max(30.);
        let overflow = indices.len() as f32 * ROW_HEIGHT > scroll_height;
        let scrollbar = if overflow {
            ui.spacing().scroll.allocated_width()
        } else {
            0.
        };
        let (header, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), HEADER_HEIGHT),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(header, 0., PANEL);
        let columns = Columns::new(egui::Rect::from_min_max(
            header.min + egui::vec2(8., 0.),
            header.max - egui::vec2(scrollbar + 8., 0.),
        ));
        let title = if self.busy {
            format!("Reading startup items…  ·  {} items", indices.len())
        } else {
            format!(
                "Application  ·  {} items  ·  {enabled} enabled",
                indices.len()
            )
        };
        let header_name =
            egui::Rect::from_min_max(columns.name.min + egui::vec2(30., 0.), columns.name.max);
        text(ui, header_name, title, MUTED, false);
        text(ui, columns.source, "Source", MUTED, false);
        text(ui, columns.scope, "Scope", MUTED, false);
        text(ui, columns.state, "State", MUTED, false);
        self.list_metrics = ListMetrics {
            total_rows: indices.len(),
            fully_visible_rows: 0,
            list_height: scroll_height,
            selected_details: self.selected.is_some(),
            first_name_left: None,
        };
        let mut action = None;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.;
            egui::ScrollArea::vertical()
                .id_salt(("startup-list", self.category()))
                .max_height(scroll_height)
                .min_scrolled_height(scroll_height)
                .auto_shrink([false, false])
                .scroll_bar_visibility(if overflow {
                    egui::scroll_area::ScrollBarVisibility::AlwaysVisible
                } else {
                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden
                })
                .show_rows(ui, ROW_HEIGHT, indices.len(), |ui, rows| {
                    if indices.is_empty() {
                        ui.add_space(24.);
                        ui.label(if self.busy {
                            "Reading startup locations…"
                        } else {
                            "No matching startup items"
                        });
                    }
                    for row in rows {
                        let index = indices[row];
                        let entry = &self.inventory.entries[index];
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), ROW_HEIGHT),
                            egui::Sense::hover(),
                        );
                        let selected = self.selected.as_ref() == Some(&entry.id);
                        let fill = if selected {
                            SURFACE
                        } else if ui.rect_contains_pointer(rect) {
                            Color32::from_rgb(29, 33, 41)
                        } else if row % 2 == 0 {
                            Color32::from_rgb(21, 24, 30)
                        } else {
                            BG
                        };
                        ui.painter().rect_filled(rect, 0., fill);
                        let columns = Columns::new(rect.shrink2(egui::vec2(8., 0.)));
                        let icon_rect = egui::Rect::from_center_size(
                            egui::pos2(columns.name.left() + 10., rect.center().y),
                            egui::vec2(20., 20.),
                        );
                        if let Some(icon) = self.icons.texture(&entry.id) {
                            ui.painter().image(
                                icon.id(),
                                icon_rect,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                                Color32::WHITE,
                            );
                        } else {
                            ui.painter().rect_stroke(
                                icon_rect.shrink(2.),
                                2.,
                                Stroke::new(1., MUTED),
                                egui::StrokeKind::Inside,
                            );
                            ui.painter().line_segment(
                                [
                                    icon_rect.left_top() + egui::vec2(2., 7.),
                                    icon_rect.right_top() + egui::vec2(-2., 7.),
                                ],
                                Stroke::new(1., MUTED),
                            );
                        }
                        let name = egui::Rect::from_min_max(
                            columns.name.min + egui::vec2(30., 0.),
                            columns.name.max,
                        );
                        // Register selection after these label UIs so their hover areas do not swallow row clicks.
                        let name_response = text(ui, name, &entry.name, TEXT, selected)
                            .on_hover_text(format!("{}\n{}", entry.name, entry.command));
                        if row == 0 {
                            self.list_metrics.first_name_left = Some(name_response.rect.left());
                        }
                        text(ui, columns.source, entry.source_name(), MUTED, false);
                        text(ui, columns.scope, &entry.scope, MUTED, false);
                        status_icon(ui.painter(), columns.state, entry.enabled);
                        let state = if entry.enabled { "Enabled" } else { "Disabled" };
                        let selection_rect = egui::Rect::from_min_max(
                            rect.min,
                            egui::pos2(columns.action.left() - 4., rect.bottom()),
                        );
                        let response = ui
                            .interact(
                                selection_rect,
                                ui.id().with(("select-entry", &entry.id)),
                                egui::Sense::click(),
                            )
                            .on_hover_text(if ui.rect_contains_pointer(columns.state) {
                                format!(
                                    "{state} — {}",
                                    if entry.enabled {
                                        "starts automatically"
                                    } else {
                                        "automatic startup is off"
                                    }
                                )
                            } else {
                                format!("{}\n{}", entry.name, entry.command)
                            });
                        if response.has_focus() {
                            ui.painter().rect_stroke(
                                selection_rect.shrink(1.),
                                2.,
                                Stroke::new(1., ACCENT),
                                egui::StrokeKind::Inside,
                            );
                        }
                        let button =
                            egui::Button::new(if entry.enabled { "Disable" } else { "Enable" })
                                .min_size(egui::vec2(70., 28.));
                        let button_rect = egui::Rect::from_center_size(
                            columns.action.center(),
                            egui::vec2(70., 28.),
                        );
                        ui.push_id(("startup-action", &entry.id), |ui| {
                            if self.busy {
                                ui.disable();
                            }
                            if ui.put(button_rect, button).clicked() {
                                action = Some((
                                    index,
                                    if entry.enabled {
                                        Operation::Disable
                                    } else {
                                        Operation::Enable
                                    },
                                ));
                            }
                        });
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::SelectableLabel,
                                true,
                                selected,
                                format!("{} — {state}", entry.name),
                            )
                        });
                        if response.clicked() {
                            self.selected = Some(entry.id.clone());
                            ui.ctx().request_repaint();
                        }
                        if ui.clip_rect().contains_rect(rect) {
                            self.list_metrics.fully_visible_rows += 1;
                        }
                    }
                });
        });
        if let Some((index, operation)) = action {
            let entry = self.inventory.entries[index].clone();
            if matches!(entry.source, Source::Task { .. } | Source::Service { .. }) {
                self.confirm = Some((entry, operation));
            } else {
                self.send(Command::Change(Box::new(entry), operation));
            }
        }
    }
    pub(super) fn message_ui(&mut self, ui: &mut egui::Ui) {
        let Some((error, message)) = self.message.clone() else {
            return;
        };
        ui.horizontal(|ui| {
            let color = if error { DANGER } else { MUTED };
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width() - 38., 24.),
                egui::Sense::hover(),
            );
            text(ui, rect, &message, color, false).on_hover_text(&message);
            if ui
                .small_button("×")
                .on_hover_text("Dismiss message")
                .clicked()
            {
                self.message = None;
            }
        });
    }
}
