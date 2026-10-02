use crate::{
    importer::trusted,
    store::{numeric_id, Result},
};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedAuth {
    pub request_url: String,
    pub token: String,
    pub athlete_id: Option<String>,
}

impl CapturedAuth {
    pub fn validate(&self) -> Result<()> {
        let url = Url::parse(&self.request_url).map_err(|_| "Invalid capture URL".to_string())?;
        if !trusted(&url)
            || self.token.is_empty()
            || self.token.len() > 16_384
            || self.token.chars().any(char::is_whitespace)
        {
            return Err("Invalid TrainingPeaks auth capture".into());
        }
        if self.athlete_id.as_ref().is_some_and(|id| !numeric_id(id)) {
            return Err("Invalid athlete ID".into());
        }
        Ok(())
    }
}

enum LoginEvent {
    Auth(CapturedAuth),
    Exit,
}

pub fn run() -> Result<()> {
    let event_loop = EventLoopBuilder::<LoginEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let parent = event_loop.create_proxy();
    std::thread::spawn(move || {
        let _ = std::io::stdin().lock().read_to_end(&mut Vec::new());
        let _ = parent.send_event(LoginEvent::Exit);
    });
    let window = WindowBuilder::new()
        .with_title("TPGPT · Sign in to TrainingPeaks")
        .with_inner_size(tao::dpi::LogicalSize::new(1120.0, 800.0))
        .build(&event_loop)
        .map_err(|e| e.to_string())?;
    let builder = wry::WebViewBuilder::new()
        .with_url("https://app.trainingpeaks.com/")
        .with_incognito(true)
        .with_initialization_script_for_main_only(include_str!("auth-capture.js"), false)
        .with_navigation_handler(|url| Url::parse(&url).is_ok_and(|url| trusted(&url)))
        .with_ipc_handler(move |request| {
            // Browser IPC only accepts credentials from TrainingPeaks origins.
            if !Url::parse(&request.uri().to_string()).is_ok_and(|url| trusted(&url)) {
                return;
            }
            if let Ok(auth) = serde_json::from_str::<CapturedAuth>(request.body()) {
                if auth.validate().is_ok() {
                    let _ = proxy.send_event(LoginEvent::Auth(auth));
                }
            }
        });
    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder
            .build_gtk(
                window
                    .default_vbox()
                    .ok_or("Login browser container unavailable")?,
            )
            .map_err(|e| e.to_string())?
    };
    #[cfg(not(target_os = "linux"))]
    let _webview = builder.build(&window).map_err(|e| e.to_string())?;
    // A second instance of this binary owns the webview's native event loop.
    // Captures go to an anonymous pipe read by the chat app, never to a file.
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(LoginEvent::Auth(auth)) => {
                if let Ok(payload) = serde_json::to_string(&auth) {
                    let mut out = std::io::stdout().lock();
                    let _ = writeln!(out, "{payload}");
                    let _ = out.flush();
                }
                // Keep the authenticated page alive for refreshed tokens.
                window.set_visible(false);
            }
            Event::UserEvent(LoginEvent::Exit) => *control_flow = ControlFlow::Exit,
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    });
}
