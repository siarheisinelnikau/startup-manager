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
        let action =
            egui::Rect::from_min_max(egui::pos2(rect.right() - 116., rect.top()), rect.max);
        let state = egui::Rect::from_min_max(
            egui::pos2(action.left() - 80., rect.top()),
            egui::pos2(action.left() - 8., rect.bottom()),
        );
        let scope = egui::Rect::from_min_max(
            egui::pos2(state.left() - 156., rect.top()),
            egui::pos2(state.left() - 8., rect.bottom()),
        );
        let source = egui::Rect::from_min_max(
            egui::pos2(scope.left() - 144., rect.top()),
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
fn status_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    enabled: bool,
    colors: &crate::appearance::Colors,
) {
    let center = rect.center();
    let stroke = Stroke::new(1.35_f32, if enabled { colors.text } else { colors.muted });
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
            && (entry.matches(&self.query)
                || self
                    .settings
                    .language
                    .text(&entry.scope)
                    .to_lowercase()
                    .contains(&self.query.to_lowercase())
                || self
                    .settings
                    .language
                    .text(entry.source_name())
                    .to_lowercase()
                    .contains(&self.query.to_lowercase()))
            && (self.filter == 0 || entry.enabled == (self.filter == 1))
    }
    pub(super) fn details_panel(&mut self, ctx: &egui::Context) {
        let language = self.settings.language;
        let colors = self.settings.theme.colors();
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
                    .fill(colors.panel)
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .stroke(Stroke::new(1.0_f32, colors.border)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 3.;
                ui.horizontal(|ui| {
                    let font = egui::TextStyle::Button.resolve(ui.style());
                    let controls_width: f32 = ["Close details", "Remove…", "Copy details"]
                        .into_iter()
                        .map(|label| {
                            ui.painter()
                                .layout_no_wrap(
                                    language.text(label).into(),
                                    font.clone(),
                                    colors.text,
                                )
                                .size()
                                .x
                                + ui.spacing().button_padding.x * 2.
                        })
                        .sum();
                    let width =
                        (ui.available_width() - controls_width - ui.spacing().item_spacing.x * 4.)
                            .max(100.);
                    let (name_rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, 28.), egui::Sense::hover());
                    text(ui, name_rect, &entry.name, colors.text, true);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(language.text("Close details")).clicked() {
                            self.selected = None;
                        }
                        if ui
                            .add_enabled(
                                !self.busy
                                    && !matches!(entry.source, Source::Service { .. })
                                    && entry.backup_id.is_none(),
                                egui::Button::new(
                                    RichText::new(language.text("Remove…")).color(colors.danger),
                                )
                                .small(),
                            )
                            .clicked()
                        {
                            self.confirm = Some((entry.clone(), Operation::Delete));
                        }
                        if ui.small_button(language.text("Copy details")).clicked() {
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
                            .color(colors.text),
                    )
                    .truncate(),
                )
                .on_hover_text(&entry.command);
                ui.add(
                    egui::Label::new(RichText::new(&entry.location).size(12.).color(colors.muted))
                        .truncate(),
                )
                .on_hover_text(&entry.location);
            });
    }
    pub(super) fn entries_ui(&mut self, ui: &mut egui::Ui) {
        let language = self.settings.language;
        let colors = self.settings.theme.colors();
        ui.horizontal(|ui| {
            let font = egui::TextStyle::Button.resolve(ui.style());
            let label_width = |text: &str| {
                ui.painter()
                    .layout_no_wrap(text.into(), font.clone(), colors.text)
                    .size()
                    .x
            };
            let state_width = ["All states", "Enabled", "Disabled"]
                .map(|label| label_width(language.text(label)))
                .into_iter()
                .fold(94., f32::max)
                + 24.;
            let padding = ui.spacing().button_padding.x * 2.;
            let mut tail = state_width
                + label_width(language.text("Refresh"))
                + padding
                + ui.spacing().item_spacing.x * 3.;
            if self.page == Page::Startup {
                tail +=
                    label_width(language.text("+ Add app")) + padding + ui.spacing().item_spacing.x;
            }
            if !self.inventory.warnings.is_empty() {
                tail += label_width(&language.format(
                    "Notices: {count}",
                    &[("count", self.inventory.warnings.len().to_string())],
                )) + padding
                    + ui.spacing().item_spacing.x;
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text(language.text("Search name, command or location"))
                    .desired_width((ui.available_width() - tail).max(100.)),
            );
            egui::ComboBox::from_id_salt("state")
                .selected_text(
                    [
                        language.text("All states"),
                        language.text("Enabled"),
                        language.text("Disabled"),
                    ][self.filter as usize],
                )
                .width(state_width)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.filter, 0, language.text("All states"));
                    ui.selectable_value(&mut self.filter, 1, language.text("Enabled"));
                    ui.selectable_value(&mut self.filter, 2, language.text("Disabled"));
                });
            if ui
                .add_enabled(!self.busy, egui::Button::new(language.text("Refresh")))
                .clicked()
            {
                self.send(Command::Scan);
            }
            if self.page == Page::Startup
                && ui
                    .add_enabled(!self.busy, egui::Button::new(language.text("+ Add app")))
                    .clicked()
            {
                self.add_open = true;
            }
            if !self.inventory.warnings.is_empty()
                && ui
                    .button(language.format(
                        "Notices: {count}",
                        &[("count", self.inventory.warnings.len().to_string())],
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
        ui.painter().rect_filled(header, 0., colors.panel);
        let columns = Columns::new(egui::Rect::from_min_max(
            header.min + egui::vec2(8., 0.),
            header.max - egui::vec2(scrollbar + 8., 0.),
        ));
        let title = if self.busy {
            language.format(
                "Reading startup items…  ·  {count} items",
                &[("count", indices.len().to_string())],
            )
        } else {
            language.format(
                "Application  ·  {count} items  ·  {enabled} enabled",
                &[
                    ("count", indices.len().to_string()),
                    ("enabled", enabled.to_string()),
                ],
            )
        };
        let header_name =
            egui::Rect::from_min_max(columns.name.min + egui::vec2(30., 0.), columns.name.max);
        text(ui, header_name, title, colors.muted, false);
        text(
            ui,
            columns.source,
            language.text("Source"),
            colors.muted,
            false,
        );
        text(
            ui,
            columns.scope,
            language.text("Scope"),
            colors.muted,
            false,
        );
        text(
            ui,
            columns.state,
            language.text("State"),
            colors.muted,
            false,
        );
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
                            language.text("Reading startup locations…")
                        } else {
                            language.text("No matching startup items")
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
                            colors.surface
                        } else if ui.rect_contains_pointer(rect) {
                            colors.hovered
                        } else if row % 2 == 0 {
                            colors.row
                        } else {
                            colors.bg
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
                                Stroke::new(1.0_f32, colors.muted),
                                egui::StrokeKind::Inside,
                            );
                            ui.painter().line_segment(
                                [
                                    icon_rect.left_top() + egui::vec2(2., 7.),
                                    icon_rect.right_top() + egui::vec2(-2., 7.),
                                ],
                                Stroke::new(1.0_f32, colors.muted),
                            );
                        }
                        let name = egui::Rect::from_min_max(
                            columns.name.min + egui::vec2(30., 0.),
                            columns.name.max,
                        );
                        // Register selection after these label UIs so their hover areas do not swallow row clicks.
                        let name_response = text(ui, name, &entry.name, colors.text, selected)
                            .on_hover_text(format!("{}\n{}", entry.name, entry.command));
                        if row == 0 {
                            self.list_metrics.first_name_left = Some(name_response.rect.left());
                        }
                        text(
                            ui,
                            columns.source,
                            language.text(entry.source_name()),
                            colors.muted,
                            false,
                        )
                        .on_hover_text(language.text(entry.source_name()));
                        text(
                            ui,
                            columns.scope,
                            language.text(&entry.scope),
                            colors.muted,
                            false,
                        )
                        .on_hover_text(language.text(&entry.scope));
                        status_icon(ui.painter(), columns.state, entry.enabled, &colors);
                        let state = if entry.enabled {
                            language.text("Enabled")
                        } else {
                            language.text("Disabled")
                        };
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
                                        language.text("starts automatically")
                                    } else {
                                        language.text("automatic startup is off")
                                    }
                                )
                            } else {
                                format!("{}\n{}", entry.name, entry.command)
                            });
                        if response.has_focus() {
                            ui.painter().rect_stroke(
                                selection_rect.shrink(1.),
                                2.,
                                Stroke::new(1.0_f32, colors.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        let button = egui::Button::new(if entry.enabled {
                            language.text("Disable")
                        } else {
                            language.text("Enable")
                        })
                        .frame(false)
                        .min_size(egui::vec2(108., 28.));
                        let button_rect = egui::Rect::from_center_size(
                            columns.action.center(),
                            egui::vec2(108., 28.),
                        );
                        ui.push_id(("startup-action", &entry.id), |ui| {
                            if self.busy {
                                ui.disable();
                            }
                            let label = format!(
                                "{} {}",
                                if entry.enabled {
                                    language.text("Disable")
                                } else {
                                    language.text("Enable")
                                },
                                entry.name
                            );
                            let response = ui.put(button_rect, button).on_hover_text(&label);
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    &label,
                                )
                            });
                            if response.hovered() {
                                ui.painter().line_segment(
                                    [
                                        egui::pos2(
                                            button_rect.left() + 12.,
                                            button_rect.bottom() - 5.,
                                        ),
                                        egui::pos2(
                                            button_rect.right() - 12.,
                                            button_rect.bottom() - 5.,
                                        ),
                                    ],
                                    Stroke::new(1.0_f32, colors.muted),
                                );
                            }
                            if response.has_focus() {
                                ui.painter().rect_stroke(
                                    button_rect,
                                    3.,
                                    Stroke::new(1.0_f32, colors.accent),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            if response.clicked() {
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
        let language = self.settings.language;
        let colors = self.settings.theme.colors();
        let Some((error, message)) = self.message.clone() else {
            return;
        };
        let message = message.render(language);
        ui.horizontal(|ui| {
            let color = if error { colors.danger } else { colors.muted };
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width() - 38., 24.),
                egui::Sense::hover(),
            );
            text(ui, rect, &message, color, false).on_hover_text(&message);
            if ui
                .small_button("×")
                .on_hover_text(language.text("Dismiss message"))
                .clicked()
            {
                self.message = None;
            }
        });
    }
}
