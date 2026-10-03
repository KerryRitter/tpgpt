use super::{view::*, Desktop};
use crate::{
    insights::{self, AnswerAction, ChartData, Metric, Totals},
    store::Result,
};
use eframe::egui::{self, Align, Color32, FontId, Frame, Layout, RichText, Stroke, Ui};
use serde_json::Value;
use std::collections::HashMap;

pub(super) enum Interaction {
    Chart(insights::ChartSpec),
    Workout(i64),
    Question(String),
}

fn card() -> Frame {
    Frame::new()
        .fill(SIDEBAR)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(16)
        .inner_margin(20)
}
fn date(text: &str) -> String {
    chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .map(|d| d.format("%b %-d").to_string())
        .unwrap_or_else(|_| text.to_owned())
}
fn distance(km: f64, miles: bool) -> f64 {
    if miles {
        km / 1.609344
    } else {
        km
    }
}
fn unit(miles: bool) -> &'static str {
    if miles {
        "mi"
    } else {
        "km"
    }
}
fn duration(hours: f64) -> String {
    let minutes = (hours.max(0.0) * 60.0).round() as u64;
    format!("{}h {:02}m", minutes / 60, minutes % 60)
}
fn change(current: f64, previous: f64) -> String {
    if previous > 0.0 {
        format!("{:+.0}% vs last week", (current / previous - 1.0) * 100.0)
    } else {
        "No prior activity recorded".into()
    }
}
fn metric_value(totals: &Totals, metric: Metric, miles: bool) -> f64 {
    match metric {
        Metric::Distance => distance(totals.distance_km, miles),
        Metric::Hours => totals.hours,
        Metric::Tss => totals.tss,
    }
}
fn metric_unit(metric: Metric, miles: bool) -> &'static str {
    match metric {
        Metric::Distance => unit(miles),
        Metric::Hours => "h",
        Metric::Tss => "TSS",
    }
}
fn stat(ui: &mut Ui, label: &str, value: String, detail: String) {
    card().inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(muted(label).size(12.0));
        ui.label(RichText::new(value).size(28.0).strong());
        ui.add(egui::Label::new(muted(detail).size(11.0)).wrap());
    });
}
fn quality(ui: &mut Ui, totals: &Totals, metric: Metric) {
    if metric == Metric::Tss && totals.tss_recorded < totals.workouts {
        ui.label(
            RichText::new(format!(
                "TSS recorded for {} of {} workouts. Missing values are unknown.",
                totals.tss_recorded, totals.workouts
            ))
            .size(12.0)
            .color(ACCENT),
        );
    }
    if totals.similar_rows > 0 {
        ui.add(egui::Label::new(RichText::new(format!("{} potentially repeated entries are included in these totals. Open workout details to compare.", totals.similar_rows)).size(12.0).color(ERROR)).wrap());
    }
}

/// Draw database-backed bars. Clicks return a range; they never send a prompt or modify records.
fn bars(ui: &mut Ui, chart: &ChartData, metric: Metric, miles: bool, height: f32) -> Option<usize> {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    let painter = ui.painter();
    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(42.0, 16.0),
        rect.max - egui::vec2(8.0, 30.0),
    );
    let max = chart
        .points
        .iter()
        .map(|p| metric_value(&p.totals, metric, miles))
        .fold(0.0, f64::max);
    let ceiling = if max > 0.0 { max * 1.18 } else { 1.0 };
    for index in 0..=3 {
        let y = plot.bottom() - plot.height() * index as f32 / 3.0;
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            Stroke::new(1.0, BORDER),
        );
        painter.text(
            egui::pos2(plot.left() - 9.0, y),
            egui::Align2::RIGHT_CENTER,
            format!(
                "{:.*}",
                usize::from(ceiling < 8.0),
                ceiling * index as f64 / 3.0
            ),
            FontId::proportional(10.0),
            MUTED,
        );
    }
    let count = chart.points.len().max(1);
    let slot = plot.width() / count as f32;
    let hovered = response
        .hover_pos()
        .filter(|p| plot.contains(*p))
        .map(|p| ((p.x - plot.left()) / slot).floor() as usize)
        .filter(|&i| i < chart.points.len());
    let stride = count
        .div_ceil((plot.width() / 65.0).max(1.0) as usize)
        .max(1);
    for (index, point) in chart.points.iter().enumerate() {
        let x = plot.left() + slot * (index as f32 + 0.5);
        let value = metric_value(&point.totals, metric, miles);
        let h = (plot.height() * (value / ceiling) as f32).max(2.0);
        let bar = egui::Rect::from_min_max(
            egui::pos2(x - slot * 0.31, plot.bottom() - h),
            egui::pos2(x + slot * 0.31, plot.bottom()),
        );
        painter.rect_filled(
            bar,
            4.0,
            if hovered == Some(index) || index + 1 == count {
                ACCENT
            } else {
                CHART_BLUE
            },
        );
        if index.is_multiple_of(stride) {
            painter.text(
                egui::pos2(x, plot.bottom() + 16.0),
                egui::Align2::CENTER_CENTER,
                date(&point.start),
                FontId::proportional(11.0),
                MUTED,
            );
        }
    }
    if let Some(index) = hovered {
        let point = &chart.points[index];
        response.clone().on_hover_ui(|ui| {
            ui.label(
                RichText::new(format!("{} – {}", date(&point.start), date(&point.end))).strong(),
            );
            ui.label(format!(
                "{:.1} {} · {} workouts",
                metric_value(&point.totals, metric, miles),
                metric_unit(metric, miles),
                point.totals.workouts
            ));
            ui.label(format!(
                "{} · {:.1} {}",
                duration(point.totals.hours),
                distance(point.totals.distance_km, miles),
                unit(miles)
            ));
            quality(ui, &point.totals, Metric::Tss);
            ui.label(muted("Click to ask about this period").size(11.0));
        });
    }
    if max == 0.0 {
        painter.text(
            plot.center(),
            egui::Align2::CENTER_CENTER,
            "No recorded values in this period",
            FontId::proportional(13.0),
            MUTED,
        );
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
        .then_some(hovered)
        .flatten()
}

impl Desktop {
    pub(super) fn home_screen(&mut self, ctx: &egui::Context) {
        let dashboard = self.dashboard.clone();
        let database = self.settings.database_path.clone();
        let mut question = None;
        let mut workout = None;
        let mut refresh = false;
        egui::CentralPanel::default().frame(Frame::new().fill(BACKGROUND).inner_margin(28)).show(ctx, |ui| {
            egui::ScrollArea::vertical().id_salt("home").auto_shrink([false,false]).show(ui, |ui| {
                let width = ui.available_width().min(1120.0);
                let gap = ((ui.available_width()-width)/2.0).max(0.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(gap);
                    ui.allocate_ui_with_layout(egui::vec2(width,0.0),Layout::top_down(Align::Min), |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("YOUR TRAINING, IN FOCUS").size(11.0).color(ACCENT).strong());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.selectable_value(&mut self.miles,false,"km");
                                ui.selectable_value(&mut self.miles,true,"mi");
                                if ui.small_button("Refresh").clicked() { refresh = true; }
                            });
                        });
                        ui.label(RichText::new("Make sense of the work.").size(34.0).strong());
                        ui.add(egui::Label::new(muted("Explore your training. Follow your curiosity. Bring the details into a conversation.").size(14.0)).wrap());
                        if self.busy {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.add(egui::Label::new(muted(&self.status).size(12.0)).truncate());
                                if ui.small_button("Return to conversation").clicked() { self.home_open = false; }
                            });
                        }
                        ui.add_space(12.0);
                        ui.horizontal_wrapped(|ui| {
                            for (name,filter) in [("All sports",""),("Run","Run"),("Bike","Bike"),("Swim","Swim")] {
                                if ui.selectable_value(&mut self.home_sport,filter.to_owned(),name).changed() { refresh = true; self.selected_period = None; }
                            }
                        });
                        ui.add_space(8.0);
                        match &dashboard {
                            None => { ui.horizontal(|ui| { ui.spinner();ui.label(muted("Reading your local training history…")); }); }
                            Some(Err(error)) => {
                                card().show(ui, |ui| {
                                    mark(ui,48.0);
                                    ui.heading("Your story starts here.");
                                    ui.label(muted("Connect TrainingPeaks to import your history, or choose an existing database in Settings."));
                                    ui.horizontal(|ui| {
                                        if ui.add_enabled(!self.busy,primary("Connect TrainingPeaks")).clicked() { self.start_login(ctx); }
                                        if ui.button("Choose database").clicked() { self.edited=self.settings.clone();self.settings_open=true; }
                                    });
                                    egui::CollapsingHeader::new("Details").show(ui,|ui| { ui.label(muted(error).size(12.0)); });
                                    ui.add_space(8.0);
                                    ui.label(muted("Import and explore only. TPGPT does not change your TrainingPeaks calendar.").size(12.0));
                                });
                            }
                            Some(Ok(home)) => {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("This week").size(20.0).strong());
                                    ui.label(muted(format!("{} – {}",date(&home.week_start),date(&home.as_of))).size(12.0));
                                });
                                let stats=[
                                    ("TRAINING TIME",duration(home.week.hours),change(home.week.hours,home.previous_week.hours)),
                                    ("DISTANCE",format!("{:.1} {}",distance(home.week.distance_km,self.miles),unit(self.miles)),change(home.week.distance_km,home.previous_week.distance_km)),
                                    ("RECORDED TSS",format!("{:.0}",home.week.tss),format!("{} / {} workouts with TSS",home.week.tss_recorded,home.week.workouts)),
                                    ("ACTIVE DAYS",home.week.active_days.to_string(),format!("{} recorded workouts",home.week.workouts)),
                                ];
                                let columns=if ui.available_width()<720.0 {2}else{4};
                                for row in stats.chunks(columns) {ui.columns(row.len(),|columns| {for (column,(label,value,detail)) in columns.iter_mut().zip(row) {stat(column,label,value.clone(),detail.clone());}});}
                                ui.add(egui::Label::new(muted("Compared with the same days last week. Recorded activity only.").size(11.0)).wrap());
                                quality(ui,&home.week,Metric::Tss);
                                ui.add_space(6.0);
                                card().show(ui,|ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(RichText::new("The bigger picture").size(19.0).strong());
                                        for metric in [Metric::Distance,Metric::Hours,Metric::Tss] { ui.selectable_value(&mut self.chart_metric,metric,metric.label()); }
                                    });
                                    ui.label(muted(format!("Eight weeks · {} · current week is partial",metric_unit(self.chart_metric,self.miles))).size(12.0));
                                    if let Some(index)=bars(ui,&home.chart,self.chart_metric,self.miles,190.0) {
                                        let p=&home.chart.points[index];self.selected_period=Some((p.start.clone(),p.end.clone()));
                                    }
                                    quality(ui,&home.chart.totals,self.chart_metric);
                                    if let Some((start,end))=&self.selected_period {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(RichText::new(format!("{} – {} selected",date(start),date(end))).color(ACCENT).size(12.0));
                                            if ui.add_enabled(!self.busy,egui::Button::new("Explore this period ↗")).clicked() { question=Some(format!("Review my {}training from {start} through {end}. What stands out compared with the previous week?",if self.home_sport.is_empty(){String::new()}else{format!("{} ",self.home_sport.to_lowercase())})); }
                                        });
                                    } else { ui.label(muted("Hover for details. Select a week to start a conversation about it.").size(11.0)); }
                                });
                                ui.add_space(6.0);
                                ui.columns(2,|columns| {
                                    card().show(&mut columns[0],|ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(RichText::new("Keep exploring").size(18.0).strong());
                                        ui.label(muted("A good question can change how you see your training.").size(12.0));
                                        for (label,prompt) in [
                                            ("Understand my recent training ↗",format!("Review my training from {} through {}. Use a chart and point me to the workouts that explain the trend.",home.chart.spec.start_date,home.as_of)),
                                            ("Compare my last two weeks ↗",format!("Compare the two weeks ending {}. Show recorded TSS and hours, flag missing or potentially duplicated data, and explain what changed.",home.as_of)),
                                            ("Find my standout workouts ↗",format!("Find my standout workouts in the last eight weeks through {}. Give me clickable workouts and explain why they matter.",home.as_of)),
                                        ] {
                                            if ui.add_enabled(!self.busy,egui::Button::new(RichText::new(label).color(ACCENT)).frame(false)).clicked() { question=Some(prompt); }
                                        }
                                        ui.add_space(6.0);
                                        ui.label(muted("Questions open in chat for you to review and send.").size(11.0));
                                    });
                                    card().show(&mut columns[1],|ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(RichText::new("Your data, at a glance").size(18.0).strong());
                                        ui.label(format!("{} imported workouts",number(home.total_workouts)));
                                        ui.label(muted(format!("Latest workout: {}",home.last_workout.as_deref().map(date).unwrap_or_else(||"None imported".into()))).size(12.0));
                                        ui.label(muted(format!("Latest import: {}",home.last_import.as_deref().map(|s|s.chars().take(16).collect::<String>().replace('T'," ")).unwrap_or_else(||"Not recorded".into()))).size(12.0));
                                        if home.last_workout.as_deref().is_some_and(|s|s<home.as_of.as_str()) { ui.label(RichText::new("Recent days may be missing. Sync to check for new activity.").color(ACCENT).size(12.0)); }
                                        ui.add_space(6.0);
                                        ui.label(muted("Local insights · TrainingPeaks stays read-only").size(11.0));
                                    });
                                });
                                ui.add_space(8.0);
                                card().show(ui,|ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new("Recent workouts").size(19.0).strong());
                                    ui.label(muted("Open a workout to explore its metrics, notes, and recorded activity.").size(12.0));
                                    if home.recent.is_empty() { ui.label(muted("No completed workouts imported in the last 60 days.")); }
                                    for row in &home.recent {
                                        let id=row["id"].as_i64().unwrap_or(0);
                                        let label=format!("{}  ·  {}   {}",date(row["date"].as_str().unwrap_or("")),row["workout_type"].as_str().unwrap_or("Workout"),row["title"].as_str().unwrap_or("Untitled workout"));
                                        ui.horizontal(|ui| {
                                            let width=(ui.available_width()-135.0).max(100.0);
                                            if ui.add_sized([width,32.0],egui::Button::new(RichText::new(label).size(13.0)).frame(false).truncate()).on_hover_text(format!("Open local workout #{id}")).clicked() { workout=Some(id); }
                                            ui.label(muted(format!("{:.1} {} · {}",distance(row["distance_meters"].as_f64().unwrap_or(0.0)/1000.0,self.miles),unit(self.miles),duration(row["duration_hours"].as_f64().unwrap_or(0.0)))).size(11.0));
                                        });
                                        ui.separator();
                                    }
                                });
                                ui.add_space(10.0);
                                ui.label(muted("Totals reflect imported records. Gaps in your history can look like rest days.").size(11.0));
                            }
                        }
                    });
                });
            });
        });
        if refresh {
            self.dashboard = None;
            self.refresh_dashboard(ctx);
        }
        if let Some(id) = workout {
            self.open_workout(&database, id, ctx);
        }
        if let Some(prompt) = question {
            self.draft_question(prompt, &database);
        }
    }

    pub(super) fn workout_details(&mut self, ctx: &egui::Context) {
        let Some(pane) = &self.workout_pane else {
            return;
        };
        let id = pane.id;
        let database = pane.database.clone();
        let result = pane.result.clone();
        let activity = pane.activity.clone();
        let selected_file = pane.activity_id;
        let mut open = true;
        let mut load_file = None;
        let mut question = false;
        egui::Window::new(format!("Workout #{id}"))
            .id(egui::Id::new("workout_details"))
            .open(&mut open)
            .default_width(790.0)
            .default_height(600.0)
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| match &result {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Reading this workout…");
                        });
                    }
                    Some(Err(error)) => {
                        ui.label(RichText::new(error).color(ERROR));
                    }
                    Some(Ok(value)) => {
                        let w = &value["workout"];
                        ui.label(
                            muted(format!(
                                "{} · {}",
                                w["workout_date"].as_str().unwrap_or(""),
                                w["workout_type"].as_str().unwrap_or("Workout")
                            ))
                            .size(12.0),
                        );
                        ui.label(
                            RichText::new(w["title"].as_str().unwrap_or("Workout"))
                                .size(25.0)
                                .strong(),
                        );
                        ui.columns(3, |columns| {
                            stat(
                                &mut columns[0],
                                "DISTANCE",
                                w["distance_meters"]
                                    .as_f64()
                                    .map(|d| {
                                        format!(
                                            "{:.2} {}",
                                            distance(d / 1000.0, self.miles),
                                            unit(self.miles)
                                        )
                                    })
                                    .unwrap_or_else(|| "Unknown".into()),
                                "Recorded distance".into(),
                            );
                            stat(
                                &mut columns[1],
                                "DURATION",
                                w["duration_hours"]
                                    .as_f64()
                                    .map(duration)
                                    .unwrap_or_else(|| "Unknown".into()),
                                "Recorded time".into(),
                            );
                            stat(
                                &mut columns[2],
                                "TSS",
                                w["tss"]
                                    .as_f64()
                                    .map(|t| format!("{t:.0}"))
                                    .unwrap_or_else(|| "Unknown".into()),
                                "Recorded training stress".into(),
                            );
                        });
                        ui.horizontal_wrapped(|ui| {
                            for (field, label, suffix) in [
                                ("heart_rate_average", "Average HR", "bpm"),
                                ("heart_rate_max", "Max HR", "bpm"),
                                ("power_average", "Average power", "W"),
                                ("rpe", "RPE", ""),
                            ] {
                                if let Some(v) = w[field].as_f64() {
                                    ui.label(muted(format!("{label}: {v:.0} {suffix}")).size(12.0));
                                }
                            }
                        });
                        ui.add_space(8.0);
                        if ui
                            .add_enabled(!self.busy, primary("Ask about this workout ↗"))
                            .clicked()
                        {
                            question = true;
                        }
                        ui.label(
                            muted("Opens a question in chat. Your workout stays unchanged.")
                                .size(11.0),
                        );
                        for (field, label) in [
                            ("description", "Workout description"),
                            ("athlete_comments", "Your notes"),
                            ("coach_comments", "Coach notes"),
                        ] {
                            if let Some(text) = w[field].as_str().filter(|s| !s.trim().is_empty()) {
                                egui::CollapsingHeader::new(label).default_open(true).show(
                                    ui,
                                    |ui| {
                                        ui.add(egui::Label::new(text).wrap().selectable(true));
                                    },
                                );
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new("Recorded activity").strong().size(18.0));
                        if let Some(files) = value["activities"].as_array() {
                            if files.is_empty() {
                                ui.label(
                                    muted("No activity file was imported for this workout.")
                                        .size(13.0),
                                );
                            }
                            for file in files {
                                let fid = file["file_id"].as_i64().unwrap_or(0);
                                let label = format!(
                                    "{} · {} laps · {} samples",
                                    file["file_format"].as_str().unwrap_or("Activity"),
                                    file["lap_count"],
                                    file["record_count"]
                                );
                                if ui
                                    .selectable_label(selected_file == Some(fid), label)
                                    .on_hover_text(file["relative_path"].as_str().unwrap_or(""))
                                    .clicked()
                                {
                                    load_file = Some(fid);
                                }
                            }
                        }
                        if selected_file.is_some() {
                            match &activity {
                                None => {
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label(muted("Decoding the local activity file…"));
                                    });
                                }
                                Some(Err(e)) => {
                                    ui.label(RichText::new(e).color(ERROR));
                                }
                                Some(Ok(value)) => {
                                    activity_ui(ui, value, self.miles);
                                }
                            }
                        }
                    }
                });
            });
        if !open {
            self.workout_pane = None;
        } else if let Some(file) = load_file {
            self.open_activity(file, ctx);
        }
        if question {
            self.workout_pane = None;
            self.draft_question(format!("Analyze workout #{id}. What stands out in its recorded metrics, notes, and available activity samples?"),&database);
        }
    }
}

pub(super) struct AnswerUi<'a> {
    pub markdown: &'a mut egui_commonmark::CommonMarkCache,
    pub charts: &'a HashMap<String, Option<Result<ChartData>>>,
    pub database: &'a str,
    pub miles: bool,
    pub actions: &'a mut Vec<Interaction>,
}
pub(super) fn answer(ui: &mut Ui, content: &str, state: AnswerUi<'_>) {
    let links = insights::links(content);
    state.markdown.link_hooks_clear();
    for url in insights::link_urls(content) {
        state.markdown.add_link_hook(url);
    }
    egui_commonmark::CommonMarkViewer::new().show(ui, state.markdown, content);
    for (url, action) in &links {
        if state.markdown.get_link_hook(url) == Some(true) {
            match action {
                AnswerAction::Workout(id) => state.actions.push(Interaction::Workout(*id)),
                AnswerAction::Chart(spec) => state.actions.push(Interaction::Chart(spec.clone())),
            }
        }
        if let AnswerAction::Chart(spec) = action {
            let key = Desktop::chart_key(state.database, spec);
            ui.add_space(12.0);
            card().inner_margin(16).show(ui,|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(spec.metric.label()).strong().size(16.0));
                    ui.label(muted(format!("{} – {}",date(&spec.start_date),date(&spec.end_date))).size(12.0));
                });
                if !spec.workout_types.is_empty() {ui.label(muted(spec.workout_types.join(" · ")).size(11.0));}
                match state.charts.get(&key) {
                    None => {state.actions.push(Interaction::Chart(spec.clone()));ui.horizontal(|ui|{ui.spinner();ui.label(muted("Reading chart data…"));});}
                    Some(None) => {ui.horizontal(|ui|{ui.spinner();ui.label(muted("Reading chart data…"));});}
                    Some(Some(Err(e))) => {ui.label(RichText::new(e).color(ERROR).size(12.0));}
                    Some(Some(Ok(chart))) => {
                        if let Some(index)=bars(ui,chart,spec.metric,state.miles,175.0) {
                            let p=&chart.points[index];
                            state.actions.push(Interaction::Question(format!("Take a closer look at my {}training from {} through {}. Which workouts explain the {} in that period?",if spec.workout_types.is_empty(){String::new()}else{format!("{} ",spec.workout_types.join(" and "))},p.start,p.end,spec.metric.label().to_lowercase())));
                        }
                        ui.label(muted(format!("{:.1} {} · {} workouts · local database",metric_value(&chart.totals,spec.metric,state.miles),metric_unit(spec.metric,state.miles),chart.totals.workouts)).size(12.0));
                        quality(ui,&chart.totals,spec.metric);
                        ui.label(muted("Hover for details. Click a bar to draft a follow-up.").size(11.0));
                    }
                }
            });
        }
    }
    let mut ids = insights::workout_references(content);
    for (_, action) in &links {
        if let AnswerAction::Workout(id) = action {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
    }
    if !ids.is_empty() {
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(muted("EXPLORE").size(10.0));
            for id in ids.into_iter().take(12) {
                if ui
                    .small_button(RichText::new(format!("Workout #{id} ↗")).color(ACCENT))
                    .clicked()
                {
                    state.actions.push(Interaction::Workout(id));
                }
            }
        });
    }
}

fn numeric(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .or_else(|| {
            value
                .as_object()
                .filter(|o| o.len() == 1)?
                .values()
                .next()
                .and_then(numeric)
        })
        .filter(|n| n.is_finite())
}
fn field(row: &Value, names: &[&str]) -> Option<f64> {
    names.iter().find_map(|name| numeric(&row[*name]))
}
fn activity_ui(ui: &mut Ui, activity: &Value, miles: bool) {
    ui.label(
        muted(format!(
            "{} total records · up to 400 sampled points",
            activity["totalRecordCount"]
        ))
        .size(11.0),
    );
    if let Some(samples) = activity["samples"].as_array() {
        let heart: Vec<_> = samples
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                field(r, &["heart_rate", "HeartRateBpm", "hr"])
                    .filter(|v| *v > 0.0)
                    .map(|v| (i as f64, v))
            })
            .collect();
        let speed: Vec<_> = samples
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                field(r, &["enhanced_speed", "speed", "Speed"])
                    .filter(|v| *v > 0.0)
                    .map(|v| (i as f64, if miles { v * 2.236936 } else { v * 3.6 }))
            })
            .collect();
        if heart.len() > 1 {
            line(ui, &heart, "Heart rate", "bpm", CHART_BLUE);
        }
        if speed.len() > 1 {
            line(
                ui,
                &speed,
                "Speed",
                if miles { "mph" } else { "km/h" },
                ACCENT,
            );
        }
        if heart.len() < 2 && speed.len() < 2 {
            ui.label(muted("This file has no usable heart-rate or speed series.").size(12.0));
        }
        ui.label(
            muted("Sample order along the workout. Hover to inspect a recorded value.").size(11.0),
        );
    }
    if let Some(laps) = activity["laps"].as_array().filter(|a| !a.is_empty()) {
        egui::CollapsingHeader::new(format!("Laps ({})", activity["totalLapCount"])).show(
            ui,
            |ui| {
                egui::Grid::new("activity_laps")
                    .striped(true)
                    .spacing([24.0, 10.0])
                    .show(ui, |ui| {
                        for h in ["Lap", "Distance", "Time", "Avg HR"] {
                            ui.label(RichText::new(h).strong().size(12.0));
                        }
                        ui.end_row();
                        for (i, lap) in laps.iter().enumerate() {
                            ui.label((i + 1).to_string());
                            ui.label(
                                field(lap, &["total_distance", "DistanceMeters"])
                                    .map(|d| {
                                        format!(
                                            "{:.2} {}",
                                            distance(d / 1000.0, miles),
                                            unit(miles)
                                        )
                                    })
                                    .unwrap_or_else(|| "—".into()),
                            );
                            ui.label(
                                field(lap, &["total_timer_time", "TotalTimeSeconds"])
                                    .map(|s| duration(s / 3600.0))
                                    .unwrap_or_else(|| "—".into()),
                            );
                            ui.label(
                                field(lap, &["avg_heart_rate", "AverageHeartRateBpm"])
                                    .map(|h| format!("{h:.0}"))
                                    .unwrap_or_else(|| "—".into()),
                            );
                            ui.end_row();
                        }
                    });
            },
        );
    }
}
fn line(ui: &mut Ui, points: &[(f64, f64)], title: &str, unit: &str, color: Color32) {
    ui.label(RichText::new(title).strong().size(14.0));
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 115.0),
        egui::Sense::hover(),
    );
    let plot = rect.shrink2(egui::vec2(38.0, 14.0));
    let min = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let a = points[0].0;
    let b = points[points.len() - 1].0;
    let position = |p: (f64, f64)| {
        egui::pos2(
            plot.left() + plot.width() * ((p.0 - a) / (b - a).max(1.0)) as f32,
            plot.bottom() - plot.height() * ((p.1 - min) / (max - min).max(1.0)) as f32,
        )
    };
    ui.painter().rect_filled(plot, 8.0, BACKGROUND);
    for value in [min, max] {
        let y = position((a, value)).y;
        ui.painter().text(
            egui::pos2(plot.left() - 5.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{value:.0}"),
            FontId::proportional(10.0),
            MUTED,
        );
    }
    ui.painter().add(egui::Shape::line(
        points.iter().copied().map(position).collect(),
        Stroke::new(1.7, color),
    ));
    if let Some(pos) = response.hover_pos().filter(|p| plot.contains(*p)) {
        let p = points
            .iter()
            .min_by(|a, b| {
                (position(**a).x - pos.x)
                    .abs()
                    .total_cmp(&(position(**b).x - pos.x).abs())
            })
            .unwrap();
        ui.painter().circle_filled(position(*p), 4.0, color);
        response.on_hover_text(format!(
            "{title}: {:.1} {unit} · sampled point {}",
            p.1,
            p.0 as u64 + 1
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workout_links_dispatch_locally_and_invalid_links_do_not_launch_a_url() {
        for (url, valid) in [
            ("tpgpt://workout/7", true),
            ("tpgpt://workout/7?delete=true", false),
        ] {
            let ctx = egui::Context::default();
            let mut markdown = egui_commonmark::CommonMarkCache::default();
            let charts = HashMap::new();
            let mut actions = vec![];
            let content = format!("[Open workout]({url})");
            let mut draw = |ctx: &egui::Context| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    answer(
                        ui,
                        &content,
                        AnswerUi {
                            markdown: &mut markdown,
                            charts: &charts,
                            database: "fixture.sqlite",
                            miles: true,
                            actions: &mut actions,
                        },
                    )
                });
            };
            let raw = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            };
            let output = ctx.run(raw(), &mut draw);
            let pos = output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(text) if text.galley.text() == "Open workout" => {
                        Some(text.pos + egui::vec2(20.0, 5.0))
                    }
                    _ => None,
                })
                .expect("rendered workout link");
            let mut input = raw();
            input.events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ];
            let _ = ctx.run(input, &mut draw);
            let mut input = raw();
            input.events = vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }];
            let output = ctx.run(input, &mut draw);
            assert!(!output
                .platform_output
                .commands
                .iter()
                .any(|c| matches!(c, egui::OutputCommand::OpenUrl(_))));
            assert_eq!(
                actions.iter().any(|a| matches!(a, Interaction::Workout(7))),
                valid
            );
        }
    }
}
