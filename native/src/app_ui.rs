use super::{CliStatus, Desktop};
use eframe::egui::{self, Align, Color32, FontId, Frame, Layout, RichText, Stroke, TextStyle, Ui};
use egui_commonmark::CommonMarkViewer;
use std::time::{Duration, Instant};

const BACKGROUND: Color32 = Color32::from_rgb(14, 20, 29);
const SIDEBAR: Color32 = Color32::from_rgb(19, 28, 39);
const SURFACE: Color32 = Color32::from_rgb(26, 37, 50);
const BORDER: Color32 = Color32::from_rgb(43, 57, 73);
const TEXT: Color32 = Color32::from_rgb(234, 240, 247);
const MUTED: Color32 = Color32::from_rgb(153, 170, 188);
const ACCENT: Color32 = Color32::from_rgb(198, 237, 142);
const TEAL: Color32 = Color32::from_rgb(121, 210, 201);
const ERROR: Color32 = Color32::from_rgb(248, 163, 145);
const READING_WIDTH: f32 = 800.0;
const CODEX_INSTALL_DOCS: &str = "https://learn.chatgpt.com/docs/codex/cli";
const CLAUDE_INSTALL_DOCS: &str = "https://code.claude.com/docs/en/quickstart";

pub(super) fn theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(24.0)),
        (TextStyle::Body, FontId::proportional(16.0)),
        (TextStyle::Button, FontId::proportional(15.0)),
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Monospace, FontId::monospace(14.0)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(12.0, 10.0);
    style.spacing.button_padding = egui::vec2(14.0, 10.0);
    style.spacing.interact_size.y = 34.0;
    style.visuals = egui::Visuals::dark();
    let visuals = &mut style.visuals;
    visuals.override_text_color = Some(TEXT);
    visuals.weak_text_color = Some(MUTED);
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = SIDEBAR;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = 16.into();
    visuals.extreme_bg_color = BACKGROUND;
    visuals.text_edit_bg_color = Some(SURFACE);
    visuals.faint_bg_color = SIDEBAR;
    visuals.code_bg_color = SURFACE;
    visuals.hyperlink_color = TEAL;
    visuals.error_fg_color = ERROR;
    visuals.selection.bg_fill = Color32::from_rgb(51, 78, 69);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
    ] {
        widget.bg_fill = SURFACE;
        widget.weak_bg_fill = SURFACE;
        widget.bg_stroke = Stroke::new(1.0, BORDER);
        widget.fg_stroke = Stroke::new(1.0, TEXT);
        widget.corner_radius = 10.into();
    }
    for widget in [&mut visuals.widgets.hovered, &mut visuals.widgets.active] {
        widget.bg_fill = Color32::from_rgb(39, 55, 68);
        widget.weak_bg_fill = widget.bg_fill;
        widget.bg_stroke = Stroke::new(1.0, TEAL);
        widget.fg_stroke = Stroke::new(1.0, TEXT);
        widget.corner_radius = 10.into();
    }
    ctx.set_style(style);
}

pub(super) fn tool_status(name: &str) -> &'static str {
    match name.trim_start_matches("mcp__trainingpeaks__") {
        "get_database_overview" => "Getting to know your training history…",
        "get_training_load" => "Looking at your training load and balance…",
        "summarize_training" => "Adding up your training…",
        "compare_training_periods" => "Comparing your training blocks…",
        "search_workouts" => "Finding relevant workouts and notes…",
        "get_workout" | "get_activity_detail" => "Reading the workout details…",
        "get_personal_records" => "Checking your strongest efforts…",
        "get_planning_context" | "draft_plan_framework" => "Building a plan around your history…",
        "save_training_plan" => "Saving your training plan…",
        "list_training_plans" | "get_training_plan" => "Looking through your saved plans…",
        _ => "Working with your training history…",
    }
}

fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).color(MUTED)
}

fn number(value: u64) -> String {
    let digits = value.to_string();
    let mut result = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(character);
    }
    result
}

fn mark(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 10.0, Color32::from_rgb(42, 57, 43));
    let point = |x, y| egui::pos2(rect.left() + size * x, rect.top() + size * y);
    painter.add(egui::Shape::line(
        vec![
            point(0.19, 0.67),
            point(0.36, 0.44),
            point(0.52, 0.56),
            point(0.78, 0.27),
        ],
        Stroke::new((size * 0.065).max(1.5), ACCENT),
    ));
    painter.circle_filled(point(0.78, 0.27), size * 0.04, ACCENT);
}

fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 16.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.0, color);
}

fn cli_badge(ui: &mut Ui, status: &CliStatus, docs: &str) -> bool {
    match status {
        CliStatus::Installed { path, version } => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(12.0, 16.0), egui::Sense::hover());
                ui.painter().add(egui::Shape::line(
                    vec![
                        rect.min + egui::vec2(1.0, 8.0),
                        rect.min + egui::vec2(5.0, 12.0),
                        rect.min + egui::vec2(11.0, 4.0),
                    ],
                    Stroke::new(2.0, ACCENT),
                ));
                ui.label(RichText::new("Installed").size(11.0).color(ACCENT))
                    .on_hover_text(format!(
                        "{version}\n{}\nSign in through the CLI before chatting.",
                        path.display()
                    ));
            });
            false
        }
        CliStatus::NotInstalled => {
            ui.hyperlink_to(RichText::new("Not installed").size(11.0).color(TEAL), docs)
                .on_hover_text("Open the official installation instructions.");
            false
        }
        CliStatus::Checking => {
            ui.label(muted("Checking…").size(11.0));
            false
        }
        CliStatus::Unavailable(error) => ui
            .add(
                egui::Button::new(RichText::new("Needs attention").size(11.0).color(ERROR))
                    .frame(false),
            )
            .on_hover_text(error)
            .clicked(),
    }
}

fn column<R>(ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.available_width().min(READING_WIDTH);
    let gap = ((ui.available_width() - width) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.add_space(gap);
        ui.allocate_ui_with_layout(egui::vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(width);
            contents(ui)
        })
        .inner
    })
    .inner
}

fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).strong().color(BACKGROUND))
        .fill(ACCENT)
        .stroke(Stroke::NONE)
        .corner_radius(10)
}

fn prompt_card(ui: &mut Ui, width: f32, index: usize, title: &str, detail: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 126.0), egui::Sense::click());
    let hovered = response.hovered();
    ui.painter().rect(
        rect,
        14.0,
        if hovered {
            Color32::from_rgb(30, 44, 56)
        } else {
            SIDEBAR
        },
        Stroke::new(1.0, if hovered { TEAL } else { BORDER }),
        egui::StrokeKind::Inside,
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink(18.0))
            .layout(Layout::top_down(Align::Min)),
    );
    child.spacing_mut().item_spacing.y = 7.0;
    child.label(
        RichText::new(format!("0{}  /  EXPLORE", index + 1))
            .size(11.0)
            .color(if index.is_multiple_of(2) {
                ACCENT
            } else {
                TEAL
            }),
    );
    child.label(RichText::new(title).strong().size(18.0));
    child.add(egui::Label::new(muted(detail).size(13.0)).wrap());
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

impl Desktop {
    pub(super) fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(270.0)
            .resizable(false)
            .frame(Frame::new().fill(SIDEBAR).inner_margin(20))
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    mark(ui, 38.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.label(RichText::new("TPGPT").size(20.0).strong());
                        ui.label(muted("Your training journal").size(12.0));
                    });
                });
                ui.add_space(24.0);
                if ui
                    .add_enabled(
                        !self.busy,
                        primary("+  New conversation")
                            .min_size(egui::vec2(ui.available_width(), 42.0)),
                    )
                    .clicked()
                {
                    self.new_chat();
                }
                ui.add_space(12.0);
                Frame::new()
                    .fill(BACKGROUND)
                    .corner_radius(12)
                    .inner_margin(14)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let connected = self.auth.is_some();
                        ui.horizontal(|ui| {
                            dot(ui, if connected { ACCENT } else { MUTED });
                            ui.label(
                                RichText::new(if connected {
                                    "TrainingPeaks connected"
                                } else {
                                    "Local history"
                                })
                                .size(13.0)
                                .color(if connected {
                                    ACCENT
                                } else {
                                    TEXT
                                }),
                            );
                        });
                        if let Some(value) = &self.overview {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(number(
                                            value["workouts"].as_u64().unwrap_or(0),
                                        ))
                                        .size(23.0)
                                        .strong(),
                                    );
                                    ui.label(muted("workouts").size(12.0));
                                });
                                ui.add_space(14.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(number(
                                            value["logical_files"].as_u64().unwrap_or(0),
                                        ))
                                        .size(23.0)
                                        .strong(),
                                    );
                                    ui.label(muted("activity files").size(12.0));
                                });
                            });
                            if let (Some(start), Some(end)) =
                                (value["first_date"].as_str(), value["last_date"].as_str())
                            {
                                let date = |text: &str| {
                                    chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                                        .map(|date| date.format("%b %Y").to_string())
                                        .unwrap_or_else(|_| text.into())
                                };
                                ui.label(
                                    muted(format!("{} – {}", date(start), date(end))).size(11.0),
                                )
                                .on_hover_text(format!("{start} to {end}"));
                            }
                        } else {
                            ui.label(muted("Connect to bring your history into focus.").size(13.0));
                        }
                        ui.add_space(4.0);
                        if connected {
                            ui.horizontal(|ui| {
                                let width = (ui.available_width() - 46.0).max(80.0);
                                if ui
                                    .add_enabled(
                                        !self.busy,
                                        egui::Button::new("Sync history")
                                            .min_size(egui::vec2(width, 34.0)),
                                    )
                                    .clicked()
                                {
                                    self.sync(ctx);
                                }
                                ui.add_enabled_ui(!self.busy, |ui| {
                                    ui.menu_button("···", |ui| {
                                        if ui.button("Sign in again").clicked() {
                                            self.start_login(ctx);
                                            ui.close();
                                        }
                                        if ui.button("Disconnect").clicked() {
                                            self.auth = None;
                                            if let Some(mut login) = self.login.take() {
                                                let _ = login.kill();
                                                let _ = login.wait();
                                            }
                                            self.status =
                                                "Disconnected. Your local history is ready.".into();
                                            ui.close();
                                        }
                                    });
                                });
                            });
                        } else if ui
                            .add_enabled(
                                !self.busy,
                                egui::Button::new("Connect TrainingPeaks")
                                    .min_size(egui::vec2(ui.available_width(), 34.0)),
                            )
                            .clicked()
                        {
                            self.start_login(ctx);
                        }
                        if self.importing {
                            ui.add(
                                egui::ProgressBar::new(self.progress)
                                    .fill(ACCENT)
                                    .show_percentage(),
                            );
                        }
                    });
                ui.add_space(20.0);
                ui.label(muted("CONVERSATIONS").size(11.0).strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.session_filter)
                        .hint_text("Search conversations…")
                        .font(FontId::proportional(13.0))
                        .margin(egui::vec2(10.0, 8.0))
                        .desired_width(f32::INFINITY),
                );
                let history_height = (ui.available_height() - 205.0).max(0.0);
                let mut selected = None;
                let filter = self.session_filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt("sessions")
                    .max_height(history_height)
                    .show(ui, |ui| {
                        ui.set_min_height(history_height);
                        ui.set_width(ui.available_width());
                        if self.sessions.is_empty() {
                            ui.add_space(8.0);
                            ui.label(muted("Your conversations will live here.").size(13.0));
                        }
                        for session in self
                            .sessions
                            .iter()
                            .filter(|session| session.title.to_lowercase().contains(&filter))
                        {
                            let current = self.selected.as_deref() == Some(&session.id);
                            let width = ui.available_width();
                            let (rect, response) = ui
                                .allocate_exact_size(egui::vec2(width, 60.0), egui::Sense::click());
                            if current || response.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    10.0,
                                    if current {
                                        Color32::from_rgb(37, 53, 49)
                                    } else {
                                        SURFACE
                                    },
                                );
                            }
                            if current {
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_size(
                                        rect.left_top() + egui::vec2(0.0, 14.0),
                                        egui::vec2(3.0, 32.0),
                                    ),
                                    2.0,
                                    ACCENT,
                                );
                            }
                            let title = if session.title == "New chat" {
                                "New conversation"
                            } else {
                                &session.title
                            };
                            ui.put(
                                egui::Rect::from_min_size(
                                    rect.min + egui::vec2(12.0, 8.0),
                                    egui::vec2(width - 24.0, 23.0),
                                ),
                                egui::Label::new(
                                    RichText::new(title).size(14.0).color(if current {
                                        TEXT
                                    } else {
                                        MUTED
                                    }),
                                )
                                .truncate(),
                            );
                            ui.painter().text(
                                rect.min + egui::vec2(12.0, 36.0),
                                egui::Align2::LEFT_TOP,
                                if session.provider == "codex" {
                                    "Codex"
                                } else {
                                    "Claude"
                                },
                                FontId::proportional(11.0),
                                if current { ACCENT } else { MUTED },
                            );
                            if response
                                .on_hover_text(&session.title)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                                && !self.busy
                            {
                                selected = Some(session.id.clone());
                            }
                        }
                    });
                if let Some(id) = selected {
                    self.select(&id);
                    self.error = None;
                    self.focus_input = true;
                }
                ui.add_space(12.0);
                ui.separator();
                ui.label(muted("ASSISTANT FOR NEW CHATS").size(11.0).strong());
                for (index, (key, label, docs)) in [
                    ("codex", "Codex", CODEX_INSTALL_DOCS),
                    ("claude", "Claude", CLAUDE_INSTALL_DOCS),
                ]
                .into_iter()
                .enumerate()
                {
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(!self.busy, |ui| {
                            ui.selectable_value(&mut self.provider, key.to_owned(), label);
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if cli_badge(ui, &self.clis[index], docs) {
                                self.setup_open = true;
                            }
                        });
                    });
                }
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(muted("Setup & sign-in").size(12.0)).frame(false))
                        .clicked()
                    {
                        self.setup_open = true;
                    }
                    if ui
                        .add(egui::Button::new(muted("Settings").size(12.0)).frame(false))
                        .clicked()
                    {
                        self.edited = self.settings.clone();
                        self.settings_open = true;
                    }
                });
            });
    }

    fn welcome(&mut self, ui: &mut Ui) {
        ui.add_space(48.0);
        ui.horizontal(|ui| {
            dot(ui, ACCENT);
            ui.label(
                RichText::new("YOUR HISTORY. YOUR NEXT CHAPTER.")
                    .size(11.0)
                    .strong()
                    .color(ACCENT),
            );
        });
        if self.clis.iter().all(|status| !status.installed()) {
            ui.add_space(8.0);
            ui.label(
                muted("To chat, install and sign in to Codex or Claude with your own account.")
                    .size(13.0),
            );
            if ui.button("Set up an assistant").clicked() {
                self.setup_open = true;
            }
        }
        ui.add_space(12.0);
        ui.label(
            RichText::new("Your training, in focus.")
                .size(36.0)
                .strong(),
        );
        ui.add_space(2.0);
        ui.add(egui::Label::new(muted("Make sense of the work you've put in. Explore your load, find patterns, and decide what comes next.").size(17.0)).wrap());
        ui.add_space(26.0);
        let prompts = [
            ("Training load", "Understand your recent load, fitness, and recovery.", "What does my recent training load look like?"),
            ("Compare blocks", "See what's changed and where you're progressing.", "Compare my training over the last eight weeks with the eight weeks before that."),
            ("Recovery signals", "Find patterns in fatigue and your workout notes.", "Find workouts where I mentioned fatigue or trouble recovering. Look for patterns in my recent load."),
            ("Plan next week", "Build a week around your training and your goals.", "Help me plan my next training week. Review my recent training, then ask me about my goals and availability."),
        ];
        let width = (ui.available_width() - 14.0) / 2.0;
        for row in 0..2 {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                for (index, &(title, detail, prompt)) in
                    prompts.iter().enumerate().skip(row * 2).take(2)
                {
                    if prompt_card(ui, width, index, title, detail) {
                        self.input = prompt.into();
                        self.focus_input = true;
                    }
                }
            });
            ui.add_space(4.0);
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            dot(ui, TEAL);
            ui.label(muted(if self.overview.is_some() { "Your local history is ready. Pick a starting point or ask your own question." } else { "Connect TrainingPeaks to import your history, or choose a database in Settings." }).size(13.0));
        });
    }

    pub(super) fn conversation(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(Frame::new().fill(BACKGROUND).inner_margin(egui::Margin::symmetric(28, 20))).show(ctx, |ui| {
            let session = self.selected.as_ref().and_then(|id| self.sessions.iter().find(|session| &session.id == id));
            let title = session.filter(|session| session.title != "New chat").map(|session| session.title.as_str()).unwrap_or("Training journal");
            let provider = session.map(|session| session.provider.as_str()).unwrap_or(&self.provider).to_owned();
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 125.0).max(100.0);
                ui.allocate_ui_with_layout(egui::vec2(width, 30.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(title).size(20.0).strong()).truncate());
                });
                Frame::new().fill(SURFACE).corner_radius(8).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
                    ui.label(RichText::new(if provider == "codex" { "Codex" } else { "Claude" }).size(12.0).color(TEAL));
                });
            });
            ui.horizontal(|ui| {
                dot(ui, TEAL);
                ui.label(muted("Grounded in your training history").size(12.0)).on_hover_text(session.map(|session| session.database_path.as_str()).unwrap_or(&self.settings.database_path));
            });
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);
            let mut retry = None;
            egui::ScrollArea::vertical().id_salt(("messages", self.selected.clone())).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                column(ui, |ui| {
                    if self.messages.is_empty() { self.welcome(ui); }
                    for index in 0..self.messages.len() {
                        let message = &self.messages[index];
                        ui.push_id(message.id, |ui| {
                            ui.add_space(14.0);
                            if message.role == "user" {
                                let max_width = (ui.available_width() * 0.87).min(650.0);
                                let text = ui.painter().layout(message.content.clone(), FontId::proportional(16.0), TEXT, max_width - 36.0);
                                let width = (text.size().x + 36.0).clamp(130.0, max_width);
                                ui.horizontal_top(|ui| {
                                    ui.add_space((ui.available_width() - width).max(0.0));
                                    Frame::new().fill(Color32::from_rgb(35, 49, 66)).corner_radius(16).inner_margin(18).show(ui, |ui| {
                                        ui.set_width(width - 36.0);
                                        ui.label(muted("YOU").size(10.0).strong());
                                        ui.add(egui::Label::new(&message.content).wrap().selectable(true));
                                    });
                                });
                                return;
                            }
                            ui.horizontal_top(|ui| {
                                mark(ui, 30.0);
                                let width = ui.available_width();
                                ui.vertical(|ui| {
                                    ui.set_width(width);
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("TPGPT").strong().size(14.0));
                                        ui.label(muted(if provider == "codex" { "with Codex" } else { "with Claude" }).size(12.0));
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if !message.content.is_empty() && message.status != "failed" {
                                                let copied = self.copied_message.is_some_and(|(id, at)| id == message.id && at.elapsed() < Duration::from_secs(2));
                                                if ui.add(egui::Button::new(muted(if copied { "Copied" } else { "Copy" }).size(11.0)).frame(false)).clicked() {
                                                    ctx.copy_text(message.content.clone());
                                                    self.copied_message = Some((message.id, Instant::now()));
                                                    ctx.request_repaint_after(Duration::from_secs(2));
                                                }
                                            }
                                        });
                                    });
                                    ui.add_space(4.0);
                                    if message.status == "failed" {
                                        let (partial, details) = message.content.split_once("\n\n[Turn interrupted: ").unwrap_or(("", &message.content));
                                        if !partial.is_empty() { CommonMarkViewer::new().show(ui, &mut self.markdown, partial); ui.add_space(12.0); }
                                        Frame::new().fill(Color32::from_rgb(49, 33, 35)).stroke(Stroke::new(1.0, Color32::from_rgb(90, 58, 58))).corner_radius(12).inner_margin(16).show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.label(RichText::new("That reply couldn't be completed").strong().color(ERROR));
                                            ui.label(muted(if details.contains("OAuth") || details.contains("authenticate") { "Sign in to your assistant's CLI again, then retry this message." } else { "Your conversation is saved. You can retry this message." }).size(13.0));
                                            egui::CollapsingHeader::new("Error details").show(ui, |ui| { ui.add(egui::Label::new(muted(details).size(12.0)).wrap().selectable(true)); });
                                            if index + 1 == self.messages.len() && ui.add_enabled(!self.busy, egui::Button::new("Retry message")).clicked() {
                                                retry = self.messages[..index].iter().rev().find(|row| row.role == "user").map(|row| row.content.clone());
                                            }
                                        });
                                    } else if message.content.is_empty() && message.status == "running" {
                                        ui.horizontal(|ui| { ui.spinner(); ui.label(muted(&self.status).size(14.0)); });
                                    } else {
                                        CommonMarkViewer::new().show(ui, &mut self.markdown, &message.content);
                                        if message.status == "interrupted" { ui.add_space(8.0); ui.label(RichText::new("Interrupted when the app closed. Send a message to continue.").size(12.0).color(ERROR)); }
                                    }
                                });
                            });
                            ui.add_space(12.0);
                        });
                    }
                    ui.add_space(16.0);
                });
            });
            if let Some(prompt) = retry { self.input = prompt; self.send(ctx); }
        });
    }

    pub(super) fn composer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("composer")
            .frame(
                Frame::new()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin::symmetric(28, 18)),
            )
            .show(ctx, |ui| {
                column(ui, |ui| {
                    if let Some(error) = self.error.clone().filter(|_| {
                        self.messages
                            .last()
                            .is_none_or(|message| message.status != "failed")
                    }) {
                        Frame::new()
                            .fill(Color32::from_rgb(49, 33, 35))
                            .corner_radius(10)
                            .inner_margin(12)
                            .show(ui, |ui| {
                                ui.horizontal_top(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width((ui.available_width() - 85.0).max(180.0));
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&error).size(13.0).color(ERROR),
                                            )
                                            .wrap()
                                            .selectable(true),
                                        );
                                    });
                                    if ui.button("Dismiss").clicked() {
                                        self.error = None;
                                    }
                                });
                            });
                        ui.add_space(8.0);
                    }
                    if self.busy {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.add(egui::Label::new(muted(&self.status).size(13.0)).truncate());
                            if let Some(started) = self.turn_started {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let seconds = started.elapsed().as_secs();
                                    ui.label(
                                        muted(format!("{}:{:02}", seconds / 60, seconds % 60))
                                            .size(12.0),
                                    );
                                });
                            }
                        });
                        ui.add_space(6.0);
                        ctx.request_repaint_after(Duration::from_millis(250));
                    }
                    let mut send = false;
                    Frame::new()
                        .fill(SURFACE)
                        .stroke(Stroke::new(1.0, BORDER))
                        .corner_radius(18)
                        .inner_margin(16)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let rows = self.input.lines().count().clamp(2, 6);
                            let editor = egui::ScrollArea::vertical()
                                .id_salt("input_scroll")
                                .max_height(150.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut self.input)
                                            .id_salt("chat_input")
                                            .hint_text("Ask anything about your training…")
                                            .font(FontId::proportional(16.0))
                                            .frame(false)
                                            .margin(egui::Margin::symmetric(2, 4))
                                            .desired_rows(rows)
                                            .desired_width(f32::INFINITY)
                                            .return_key(egui::KeyboardShortcut::new(
                                                egui::Modifiers::SHIFT,
                                                egui::Key::Enter,
                                            )),
                                    )
                                })
                                .inner;
                            if self.focus_input {
                                editor.request_focus();
                                self.focus_input = false;
                            }
                            let shortcut = editor.has_focus()
                                && ctx.input(|input| {
                                    input.key_pressed(egui::Key::Enter) && !input.modifiers.shift
                                });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    muted("Enter to send · Shift + Enter for a new line")
                                        .size(11.0),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if self.busy {
                                        if ui
                                            .add(
                                                egui::Button::new(if self.importing {
                                                    "Stop sync"
                                                } else {
                                                    "Stop"
                                                })
                                                .min_size(egui::vec2(82.0, 34.0)),
                                            )
                                            .clicked()
                                        {
                                            if let Some(cancel) = &self.cancel {
                                                let _ = cancel.send(true);
                                            }
                                        }
                                    } else if ui
                                        .add_enabled(
                                            !self.input.trim().is_empty()
                                                && self.current_cli_status().installed(),
                                            primary("Send").min_size(egui::vec2(82.0, 34.0)),
                                        )
                                        .clicked()
                                    {
                                        send = true;
                                    }
                                });
                            });
                            send |= shortcut;
                        });
                    ui.add_space(2.0);
                    let provider = self
                        .selected
                        .as_ref()
                        .and_then(|id| self.sessions.iter().find(|session| &session.id == id))
                        .map(|session| session.provider.as_str())
                        .unwrap_or(&self.provider);
                    ui.horizontal(|ui| {
                        ui.label(
                            muted(format!(
                                "Replies use your local history and your {} account.",
                                if provider == "codex" {
                                    "Codex"
                                } else {
                                    "Claude"
                                }
                            ))
                            .size(11.0),
                        );
                    });
                    if send {
                        self.send(ctx);
                    }
                });
            });
    }

    pub(super) fn assistant_setup(&mut self, ctx: &egui::Context) {
        if !self.setup_open {
            return;
        }
        let mut open = true;
        let mut recheck = false;
        let mut selected = None;
        egui::Window::new("Assistant setup").open(&mut open).collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(590.0).resizable(false).show(ctx, |ui| {
                ui.label(RichText::new("Choose the account you already use").size(19.0).strong());
                ui.add(egui::Label::new(muted("Both assistants can work with your training history. Install one CLI, sign in from a terminal, then start a conversation here.").size(14.0)).wrap());
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 210.0).clamp(240.0, 540.0)).show(ui, |ui| {
                    for (index, (key, label, account, docs, login)) in [
                        ("codex", "Codex", "Uses your OpenAI / ChatGPT sign-in.", CODEX_INSTALL_DOCS, "codex login"),
                        ("claude", "Claude Code", "Uses your Claude or Anthropic Console account.", CLAUDE_INSTALL_DOCS, "claude"),
                    ].into_iter().enumerate() {
                        Frame::new().fill(BACKGROUND).stroke(Stroke::new(1.0, BORDER)).corner_radius(12).inner_margin(16).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(label).size(18.0).strong());
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| { cli_badge(ui, &self.clis[index], docs); });
                            });
                            ui.label(muted(account).size(13.0));
                            if let CliStatus::Unavailable(error) = &self.clis[index] { ui.add(egui::Label::new(RichText::new(error).size(12.0).color(ERROR)).wrap().selectable(true)); }
                            ui.horizontal(|ui| {
                                ui.label(muted("1. Install").size(13.0));
                                ui.hyperlink_to("Official installation docs", docs);
                            });
                            ui.label(muted("2. Sign in from a terminal").size(13.0));
                            ui.horizontal(|ui| {
                                ui.code(login);
                                if ui.small_button("Copy command").clicked() { ctx.copy_text(login.to_owned()); }
                            });
                            if key == "claude" { ui.add(egui::Label::new(muted("Claude prompts you to sign in on first use. To sign in again, type /login inside Claude Code.").size(12.0)).wrap()); }
                            if ui.add_enabled(!self.busy && self.clis[index].installed(), egui::Button::new(format!("Use {} for new conversations", if key == "codex" { "Codex" } else { "Claude" }))).clicked() { selected = Some(key.to_owned()); }
                        });
                        ui.add_space(8.0);
                    }
                });
                ui.add(egui::Label::new(muted("The green check confirms the CLI can run. Sign-in happens in the CLI; TPGPT uses that account when you chat.").size(12.0)).wrap());
                ui.horizontal(|ui| {
                    let checking = self.clis.iter().any(|status| matches!(status, CliStatus::Checking));
                    if ui.add_enabled(!checking, egui::Button::new("Recheck installations")).clicked() { recheck = true; }
                    if checking { ui.spinner(); }
                });
                ui.label(muted("Installed somewhere else? Set the executable paths in Settings. Existing conversations keep their assistant.").size(12.0));
            });
        if recheck {
            self.check_clis(ctx);
        }
        if let Some(provider) = selected {
            self.provider = provider;
            open = false;
        }
        self.setup_open = open;
    }
}
