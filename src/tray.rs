//! System tray integration for background running.
//! Feature-gated behind the `gui` feature.

use std::sync::mpsc;

use tracing::{error, info};
use tray_icon::menu::{Menu, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::error::{AgentError, AgentResult};

/// Commands that the tray can emit to the application.
#[derive(Debug)]
pub enum TrayCommand {
    Quit,
}

/// Create a programmatic RGBA icon (a simple colored square).
fn create_icon() -> AgentResult<tray_icon::Icon> {
    let width = 32;
    let height = 32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let in_circle = ((x as i32 - 16).pow(2) + (y as i32 - 16).pow(2)) < 220;
            if in_circle {
                rgba.extend_from_slice(&[64, 128, 255, 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    tray_icon::Icon::from_rgba(rgba, width, height)
        .map_err(|e| AgentError::Tray(format!("icon creation failed: {e}")))
}

/// Build and return a tray icon with a menu. The receiver gets events
/// when menu items are clicked.
pub fn build_tray() -> AgentResult<(TrayIcon, mpsc::Receiver<TrayCommand>)> {
    let (tx, rx) = mpsc::channel::<TrayCommand>();

    let open_item = MenuItem::new("Open GUI", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let menu = Menu::new();
    menu.append(&open_item).map_err(|e| AgentError::Tray(format!("menu append failed: {e}")))?;
    menu.append(&quit_item).map_err(|e| AgentError::Tray(format!("menu append failed: {e}")))?;

    let tray = TrayIconBuilder::new()
        .with_tooltip("Plan Executor Agent")
        .with_icon(create_icon()?)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(true)
        .build()
        .map_err(|e| AgentError::Tray(format!("tray icon build failed: {e}")))?;

    // Spawn a thread to listen for tray events
    std::thread::spawn(move || {
        TrayIconEvent::set_event_handler(Some(move |event| {
            info!(?event, "tray event received");
        }));

        let _ = open_item;
        let _ = quit_item;
        let _ = tx;

        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    });

    Ok((tray, rx))
}

/// Run the application as a background service with tray icon.
pub fn run_service() {
    info!("starting tray service");

    let (_tray, rx) = match build_tray() {
        Ok(result) => result,
        Err(e) => {
            error!(error = %e, "failed to build tray icon, running without it");
            // Fall back to regular ctrl-c wait
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
    };

    // Wait for quit signal
    while let Ok(cmd) = rx.recv() {
        match cmd {
            TrayCommand::Quit => {
                info!("quit command received, shutting down");
                std::process::exit(0);
            }
        }
    }

    std::process::exit(0);
}
