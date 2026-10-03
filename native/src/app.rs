use crate::{
    browser::CapturedAuth,
    data,
    importer::{self, ImportConfig},
    insights::{self, ChartData, ChartSpec, Dashboard},
    process::{self, ChatStream, CliStatus},
    store::{ChatStore, Message, Result, Session, Settings},
};
use eframe::egui;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};
use tokio::sync::watch;

#[path = "home_ui.rs"]
mod home;
#[path = "app_ui.rs"]
mod view;

struct WorkoutPane {
    database: String,
    id: i64,
    result: Option<Result<Value>>,
    activity_id: Option<i64>,
    activity: Option<Result<Value>>,
}

enum Event {
    Auth(CapturedAuth),
    LoginClosed,
    Progress(Value),
    ImportDone(Result<Value>),
    Text {
        message: i64,
        delta: String,
    },
    ProviderId {
        session: String,
        id: String,
    },
    ChatDone {
        message: i64,
        content: String,
        result: Result<()>,
    },
    Overview(Result<Value>),
    CliChecked {
        generation: u64,
        index: usize,
        status: CliStatus,
    },
    Dashboard {
        generation: u64,
        result: Box<Result<Dashboard>>,
    },
    Chart {
        key: String,
        result: Result<ChartData>,
    },
    Workout {
        database: String,
        id: i64,
        result: Result<Value>,
    },
    Activity {
        database: String,
        id: i64,
        file_id: i64,
        result: Result<Value>,
    },
}

struct Desktop {
    root: PathBuf,
    settings: Settings,
    edited: Settings,
    store: ChatStore,
    sessions: Vec<Session>,
    selected: Option<String>,
    messages: Vec<Message>,
    input: String,
    provider: String,
    status: String,
    error: Option<String>,
    overview: Option<Value>,
    auth: Option<CapturedAuth>,
    login: Option<Child>,
    settings_open: bool,
    busy: bool,
    importing: bool,
    cancel: Option<watch::Sender<bool>>,
    progress: f32,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
    runtime: Option<tokio::runtime::Runtime>,
    markdown: egui_commonmark::CommonMarkCache,
    session_filter: String,
    focus_input: bool,
    turn_started: Option<std::time::Instant>,
    copied_message: Option<(i64, std::time::Instant)>,
    clis: [CliStatus; 2],
    cli_generation: u64,
    setup_open: bool,
    home_open: bool,
    dashboard: Option<Result<Dashboard>>,
    dashboard_generation: u64,
    home_sport: String,
    chart_metric: insights::Metric,
    miles: bool,
    selected_period: Option<(String, String)>,
    charts: HashMap<String, Option<Result<ChartData>>>,
    workout_pane: Option<WorkoutPane>,
}

pub fn run(login: bool) -> Result<()> {
    // Keep the original storage identifier so renaming the app preserves existing data.
    let default_root = directories::ProjectDirs::from("com", "KerryRitter", "TrainingPeaks Chat")
        .ok_or("Cannot locate app data directory")?
        .data_local_dir()
        .to_path_buf();
    let root = std::env::var_os("TRAININGPEAKS_APP_DATA")
        .map(PathBuf::from)
        .unwrap_or(default_root);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let settings = Settings::load(&root)?;
    settings.validate()?;
    let store = ChatStore::open(&root.join("chat.sqlite"))?;
    let sessions = store.list()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let (sender, receiver) = mpsc::channel();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 900.0])
            .with_min_inner_size([860.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "TPGPT",
        options,
        Box::new(move |cc| {
            view::theme(&cc.egui_ctx);
            let mut app = Desktop {
                root,
                edited: settings.clone(),
                settings,
                store,
                sessions,
                selected: None,
                messages: vec![],
                input: String::new(),
                provider: "codex".into(),
                status: "Sign in to import, or select an existing database in Settings.".into(),
                error: None,
                overview: None,
                auth: None,
                login: None,
                settings_open: false,
                busy: false,
                importing: false,
                cancel: None,
                progress: 0.0,
                sender,
                receiver,
                runtime: Some(runtime),
                markdown: egui_commonmark::CommonMarkCache::default(),
                session_filter: String::new(),
                focus_input: true,
                turn_started: None,
                copied_message: None,
                clis: [CliStatus::Checking, CliStatus::Checking],
                cli_generation: 0,
                setup_open: false,
                home_open: true,
                dashboard: None,
                dashboard_generation: 0,
                home_sport: String::new(),
                chart_metric: insights::Metric::Distance,
                miles: true,
                selected_period: None,
                charts: HashMap::new(),
                workout_pane: None,
            };
            if let Some(id) = app.sessions.first().map(|session| session.id.clone()) {
                app.select(&id);
            }
            app.refresh_overview(&cc.egui_ctx);
            app.check_clis(&cc.egui_ctx);
            if login {
                app.start_login(&cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())
}

impl Desktop {
    fn check_clis(&mut self, ctx: &egui::Context) {
        self.cli_generation += 1;
        self.clis = [CliStatus::Checking, CliStatus::Checking];
        for (index, name) in [
            self.settings.codex_path.clone(),
            self.settings.claude_path.clone(),
        ]
        .into_iter()
        .enumerate()
        {
            let sender = self.sender.clone();
            let ctx = ctx.clone();
            let generation = self.cli_generation;
            self.runtime.as_ref().unwrap().spawn(async move {
                let status = process::probe_cli(&name).await;
                let _ = sender.send(Event::CliChecked {
                    generation,
                    index,
                    status,
                });
                ctx.request_repaint();
            });
        }
    }
    fn current_cli_status(&self) -> &CliStatus {
        let provider = self
            .selected
            .as_ref()
            .and_then(|id| self.sessions.iter().find(|session| &session.id == id))
            .map(|session| session.provider.as_str())
            .unwrap_or(&self.provider);
        &self.clis[usize::from(provider == "claude")]
    }
    fn fail(&mut self, error: String) {
        self.error = Some(error);
        if !self.busy {
            self.status = "That request needs attention. You can try again.".into();
        }
    }
    fn select(&mut self, id: &str) {
        match self.store.messages(id) {
            Ok(messages) => {
                self.selected = Some(id.into());
                self.messages = messages;
            }
            Err(error) => self.fail(error),
        }
    }
    fn refresh_sessions(&mut self) {
        match self.store.list() {
            Ok(sessions) => self.sessions = sessions,
            Err(error) => self.fail(error),
        }
    }
    fn refresh_overview(&mut self, ctx: &egui::Context) {
        self.refresh_dashboard(ctx);
        let database = self.settings.database_path.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        self.runtime.as_ref().unwrap().spawn(async move {
            let result = data::open_readonly(std::path::Path::new(&database))
                .and_then(|db| data::overview(&db));
            let _ = sender.send(Event::Overview(result));
            ctx.request_repaint();
        });
    }
    fn refresh_dashboard(&mut self, ctx: &egui::Context) {
        self.dashboard_generation += 1;
        let generation = self.dashboard_generation;
        let database = self.settings.database_path.clone();
        let sport = self.home_sport.clone();
        let zone = self
            .settings
            .time_zone
            .parse::<chrono_tz::Tz>()
            .unwrap_or(chrono_tz::UTC);
        let today = chrono::Utc::now().with_timezone(&zone).date_naive();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        self.runtime.as_ref().unwrap().spawn(async move {
            let result = data::open_readonly(std::path::Path::new(&database))
                .and_then(|db| insights::dashboard(&db, today, &sport));
            let _ = sender.send(Event::Dashboard {
                generation,
                result: Box::new(result),
            });
            ctx.request_repaint();
        });
    }
    fn chart_key(database: &str, spec: &ChartSpec) -> String {
        format!("{database}\0{}", spec.url())
    }
    fn request_chart(&mut self, database: &str, spec: &ChartSpec, ctx: &egui::Context) {
        let key = Self::chart_key(database, spec);
        if self.charts.contains_key(&key) {
            return;
        }
        self.charts.insert(key.clone(), None);
        let database = database.to_owned();
        let spec = spec.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        self.runtime.as_ref().unwrap().spawn(async move {
            let result = data::open_readonly(std::path::Path::new(&database))
                .and_then(|db| insights::chart(&db, spec));
            let _ = sender.send(Event::Chart { key, result });
            ctx.request_repaint();
        });
    }
    fn open_workout(&mut self, database: &str, id: i64, ctx: &egui::Context) {
        let database = database.to_owned();
        self.workout_pane = Some(WorkoutPane {
            database: database.clone(),
            id,
            result: None,
            activity_id: None,
            activity: None,
        });
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        self.runtime.as_ref().unwrap().spawn(async move {
            let result = data::open_readonly(std::path::Path::new(&database))
                .and_then(|db| insights::workout(&db, id));
            let _ = sender.send(Event::Workout {
                database,
                id,
                result,
            });
            ctx.request_repaint();
        });
    }
    fn open_activity(&mut self, file_id: i64, ctx: &egui::Context) {
        let Some(pane) = self.workout_pane.as_mut() else {
            return;
        };
        pane.activity_id = Some(file_id);
        pane.activity = None;
        let database = pane.database.clone();
        let id = pane.id;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        self.runtime.as_ref().unwrap().spawn(async move {
            let result = data::open_readonly(std::path::Path::new(&database))
                .and_then(|db| data::activity(&db, file_id, 400));
            let _ = sender.send(Event::Activity {
                database,
                id,
                file_id,
                result,
            });
            ctx.request_repaint();
        });
    }
    fn draft_question(&mut self, question: String, database: &str) {
        if self.busy {
            return;
        }
        let matching = self
            .selected
            .as_ref()
            .and_then(|id| self.store.get(id).ok())
            .is_some_and(|s| s.database_path == database);
        if !matching {
            match self.store.create(&self.provider, database) {
                Ok(session) => {
                    self.select(&session.id);
                    self.refresh_sessions();
                }
                Err(error) => {
                    self.fail(error);
                    return;
                }
            }
        }
        self.home_open = false;
        self.input = question;
        self.focus_input = true;
    }
    fn start_login(&mut self, ctx: &egui::Context) {
        if let Some(mut child) = self.login.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let executable = match std::env::current_exe() {
            Ok(path) => path,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        };
        let mut command = std::process::Command::new(executable);
        command
            .arg("--login")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::piped());
        command.env_remove("TRAININGPEAKS_TOKEN");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        match command.spawn() {
            Ok(mut child) => {
                let stdout = child.stdout.take().unwrap();
                let sender = self.sender.clone();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        let Ok(line) = line else {
                            break;
                        };
                        if let Ok(auth) = serde_json::from_str::<CapturedAuth>(&line) {
                            if auth.validate().is_ok() {
                                let _ = sender.send(Event::Auth(auth));
                                ctx.request_repaint();
                            }
                        }
                    }
                    let _ = sender.send(Event::LoginClosed);
                    ctx.request_repaint();
                });
                self.login = Some(child);
                self.status =
                    "Sign in in the TrainingPeaks window. Auth is detected automatically.".into();
                self.error = None;
            }
            Err(error) => self.fail(format!(
                "Could not open the TrainingPeaks login browser: {error}"
            )),
        }
    }
    fn sync(&mut self, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        let Some(auth) = self.auth.as_ref() else {
            self.fail("Sign in to TrainingPeaks first".into());
            return;
        };
        if self.settings.athlete_id.is_empty() {
            self.fail(
                "Set the numeric athlete ID in Settings. Automatic detection has not found it yet."
                    .into(),
            );
            self.settings_open = true;
            return;
        }
        let config = ImportConfig {
            database: self.settings.database_path.clone().into(),
            cache: self.root.join("exports"),
            athlete: self.settings.athlete_id.clone(),
            token: auth.token.clone(),
            end: chrono::Local::now().date_naive().to_string(),
            years: self.settings.years,
            months: self.settings.months_per_request,
            force: false,
            time_zone: self.settings.time_zone.clone(),
        };
        let (cancel, mut signal) = watch::channel(false);
        self.cancel = Some(cancel);
        self.busy = true;
        self.importing = true;
        self.progress = 0.0;
        self.turn_started = Some(std::time::Instant::now());
        self.error = None;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        self.runtime.as_ref().unwrap().spawn(async move{
            let progress_sender=sender.clone();let progress_ctx=ctx.clone();
            let result=tokio::select!{
                result=importer::sync(config,move |value|{let _=progress_sender.send(Event::Progress(value));progress_ctx.request_repaint();})=>result,
                _=signal.changed()=>Err("Import cancelled. Completed windows are saved; sync again to resume.".into()),
            };
            let _=sender.send(Event::ImportDone(result));ctx.request_repaint();
        });
    }
    fn new_chat(&mut self) {
        if self.busy {
            return;
        }
        match self
            .store
            .create(&self.provider, &self.settings.database_path)
        {
            Ok(session) => {
                self.home_open = false;
                self.selected = Some(session.id);
                self.messages.clear();
                self.input.clear();
                self.error = None;
                self.focus_input = true;
                self.status = "Ready for your next question.".into();
                self.refresh_sessions();
            }
            Err(error) => self.fail(error),
        }
    }
    fn send(&mut self, ctx: &egui::Context) {
        if self.busy || self.input.trim().is_empty() {
            return;
        }
        if !self.current_cli_status().installed() {
            self.setup_open = true;
            return;
        }
        let prompt = self.input.trim().to_owned();
        if prompt.len() > 100_000 {
            self.fail("Message exceeds 100,000 bytes".into());
            return;
        }
        if self.selected.is_none() {
            self.new_chat();
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        let session = match self.store.get(&id) {
            Ok(session) => session,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        if !std::path::Path::new(&session.database_path).is_file() {
            self.fail("Import history first, or choose an existing SQLite database in Settings and start a new chat.".into());
            return;
        }
        let executable = if session.provider == "codex" {
            &self.settings.codex_path
        } else {
            &self.settings.claude_path
        };
        let path = process::executable(executable);
        if path.components().count() > 1 && !path.is_file() {
            self.fail("CLI executable not found. Check its path in Settings.".into());
            return;
        }
        let program = match std::env::current_exe() {
            Ok(path) => path,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        };
        let working = self.root.join("sessions").join(&session.id);
        if let Err(error) = std::fs::create_dir_all(&working) {
            self.fail(error.to_string());
            return;
        }
        let message = match self
            .store
            .add(&id, "user", &prompt, "complete")
            .and_then(|_| self.store.add(&id, "assistant", "", "running"))
        {
            Ok(message) => message,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        self.input.clear();
        self.select(&id);
        self.refresh_sessions();
        self.busy = true;
        self.importing = false;
        self.error = None;
        self.status = format!("{} is thinking…", session.provider);
        self.turn_started = Some(std::time::Instant::now());
        let mut command = tokio::process::Command::new(path);
        command
            .args(process::cli_arguments(&session, &self.settings, &program))
            .current_dir(working)
            .env_remove("CLAUDECODE");
        let prompt = process::instructions(&prompt, session.provider_session_id.is_none());
        let (cancel, signal) = watch::channel(false);
        self.cancel = Some(cancel);
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        self.runtime.as_ref().unwrap().spawn(async move{
            let mut stream=ChatStream::default();let mut known_id=session.provider_session_id.clone();
            let result=process::run_json_process(command,&prompt,signal,Duration::from_secs(1200),|value|{
                let event=stream.accept(&session.provider,&value);
                if stream.provider_id.is_some()&&stream.provider_id!=known_id{
                    let id=stream.provider_id.clone().unwrap();known_id=Some(id.clone());let _=sender.send(Event::ProviderId{session:session.id.clone(),id});
                }
                if let Some(event)=event{if event["type"]=="text"{let _=sender.send(Event::Text{message,delta:event["delta"].as_str().unwrap_or("").into()});}else{let _=sender.send(Event::Progress(json!({"message":view::tool_status(event["message"].as_str().unwrap_or(""))})));}ctx.request_repaint();}
                Ok(())
            }).await;
            let result=stream.finish(result);
            let _=sender.send(Event::ChatDone{message,content:stream.text,result});ctx.request_repaint();
        });
    }
    fn events(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Auth(auth) => {
                    if let Some(id) = &auth.athlete_id {
                        self.settings.athlete_id = id.clone();
                        self.edited.athlete_id = id.clone();
                        let _ = self.settings.save(&self.root);
                    }
                    self.auth = Some(auth);
                    self.status =
                        "TrainingPeaks connected. Sync history to import your data.".into();
                    self.error = None;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Event::LoginClosed => {
                    if self.auth.is_none() {
                        self.status = "Login window closed. Use Sign in to try again.".into();
                    }
                }
                Event::Progress(value) => {
                    if let Some(message) = value["message"].as_str() {
                        self.status = message.into();
                    }
                    if let (Some(done), Some(total)) =
                        (value["done"].as_f64(), value["total"].as_f64())
                    {
                        if total > 0.0 {
                            self.progress = (done / total) as f32;
                        }
                    }
                }
                Event::ImportDone(result) => {
                    self.busy = false;
                    self.importing = false;
                    self.turn_started = None;
                    self.cancel = None;
                    match result {
                        Ok(value) => {
                            self.charts.clear();
                            self.refresh_dashboard(ctx);
                            self.overview = Some(value["overview"].clone());
                            self.status = format!(
                                "History ready: {} exports imported, {} already complete.",
                                value["completed"], value["skipped"]
                            );
                        }
                        Err(error) => {
                            if error.starts_with("AUTH_EXPIRED") {
                                self.auth = None;
                            }
                            self.fail(error);
                            self.refresh_overview(ctx);
                        }
                    }
                }
                Event::Text { message, delta } => {
                    if let Some(row) = self.messages.iter_mut().find(|row| row.id == message) {
                        row.content.push_str(&delta);
                        changed = true;
                    }
                }
                Event::ProviderId { session, id } => {
                    if let Err(error) = self.store.set_provider_id(&session, &id) {
                        self.fail(error);
                    }
                    self.refresh_sessions();
                }
                Event::ChatDone {
                    message,
                    content,
                    result,
                } => {
                    self.busy = false;
                    self.turn_started = None;
                    self.cancel = None;
                    let status = if result.is_ok() { "complete" } else { "failed" };
                    let text = match &result {
                        Ok(()) => content,
                        Err(error) if content.is_empty() => error.clone(),
                        Err(error) => format!("{content}\n\n[Turn interrupted: {error}]"),
                    };
                    if let Err(error) = self.store.finish(message, &text, status) {
                        self.fail(error);
                    }
                    if let Err(error) = result {
                        self.fail(error);
                    } else {
                        self.status = "Ready for your next question.".into();
                    }
                    if let Some(id) = self.selected.clone() {
                        self.select(&id);
                    }
                    self.refresh_sessions();
                    changed = false;
                }
                Event::Overview(result) => match result {
                    Ok(value) => self.overview = Some(value),
                    Err(_) => self.overview = None,
                },
                Event::CliChecked {
                    generation,
                    index,
                    status,
                } => {
                    if generation == self.cli_generation {
                        self.clis[index] = status;
                    }
                }
                Event::Dashboard { generation, result } => {
                    if generation == self.dashboard_generation {
                        self.dashboard = Some(*result);
                    }
                }
                Event::Chart { key, result } => {
                    if self.charts.contains_key(&key) {
                        self.charts.insert(key, Some(result));
                    }
                }
                Event::Workout {
                    database,
                    id,
                    result,
                } => {
                    if let Some(pane) = self
                        .workout_pane
                        .as_mut()
                        .filter(|p| p.database == database && p.id == id)
                    {
                        pane.result = Some(result);
                    }
                }
                Event::Activity {
                    database,
                    id,
                    file_id,
                    result,
                } => {
                    if let Some(pane) = self.workout_pane.as_mut().filter(|p| {
                        p.database == database && p.id == id && p.activity_id == Some(file_id)
                    }) {
                        pane.activity = Some(result);
                    }
                }
            }
        }
        if changed {
            for row in self.messages.iter().filter(|row| row.status == "running") {
                let _ = self.store.finish(row.id, &row.content, "running");
            }
        }
    }
    fn settings_ui(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        let mut save = false;
        egui::Window::new("Settings").open(&mut open).resizable(true).default_width(570.0).show(ctx,|ui|{
            ui.label("New chats use these settings. Existing chats keep their provider and database.");
            ui.add_enabled_ui(!self.busy,|ui|{
                for (label,value) in [("SQLite database path",&mut self.edited.database_path),("Athlete ID",&mut self.edited.athlete_id),("Timezone",&mut self.edited.time_zone),("Codex executable",&mut self.edited.codex_path),("Claude executable",&mut self.edited.claude_path)]{ui.label(label);ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));}
                ui.horizontal(|ui|{ui.label("History (years)");ui.add(egui::DragValue::new(&mut self.edited.years).range(1..=50));ui.label("Months per export");ui.add(egui::DragValue::new(&mut self.edited.months_per_request).range(1..=12));});
                if ui.button("Save settings").clicked(){save=true;}
            });
            ui.small(format!("App data: {}",self.root.display()));
        });
        if save {
            match self.edited.save(&self.root) {
                Ok(()) => {
                    self.settings = self.edited.clone();
                    self.overview = None;
                    self.refresh_overview(ctx);
                    self.check_clis(ctx);
                    self.status =
                        "Settings saved. Start a new chat to use a different database.".into();
                    self.settings_open = false;
                }
                Err(error) => self.fail(error),
            }
        } else {
            self.settings_open = open;
        }
    }
}

impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.events(ctx);
        self.sidebar(ctx);
        if self.home_open {
            self.home_screen(ctx);
        } else {
            self.composer(ctx);
            self.conversation(ctx);
        }
        self.workout_details(ctx);
        self.settings_ui(ctx);
        self.assistant_setup(ctx);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(true);
        }
        if let Some(mut login) = self.login.take() {
            let _ = login.kill();
            let _ = login.wait();
        }
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::from_secs(2));
        }
    }
}
