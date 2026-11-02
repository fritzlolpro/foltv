mod config;
mod engine;
mod input;
mod library;
mod mpv;
mod state;

use crate::engine::{Command, PlayerEvent};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

enum AppMsg {
    Cmd(Command),
    Ev(PlayerEvent),
}

fn main() {
    if let Err(err) = run() {
        eprintln!("foltv: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("foltv.toml"));
    let state_path = config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("foltv-state.json");

    let config = config::load(&config_path)?;
    let mut engine = engine::Engine::new(config, state::load(&state_path));
    let mut mpv = mpv::Mpv::spawn()?;
    engine.start(&mut mpv).inspect_err(|_| mpv.kill())?;

    let (app_tx, app_rx) = mpsc::channel::<AppMsg>();

    let key_tx = app_tx.clone();
    std::thread::spawn(move || {
        if let Err(err) = input::read_commands(|cmd| {
            let _ = key_tx.send(AppMsg::Cmd(cmd));
        }) {
            eprintln!("foltv: input stopped: {err:#}");
        }
    });

    let event_tx = app_tx.clone();
    if let Some(events) = mpv.take_events() {
        std::thread::spawn(move || {
            for event in events {
                let _ = event_tx.send(AppMsg::Ev(event));
            }
        });
    }
    drop(app_tx);

    while let Ok(msg) = app_rx.recv() {
        match msg {
            AppMsg::Cmd(cmd) => {
                if !engine.on_command(cmd, &mut mpv) {
                    break;
                }
            }
            AppMsg::Ev(event) => engine.on_player_event(event, &mut mpv),
        }
        // spec: persist on every change — power may be cut at any moment
        let _ = state::save(&state_path, engine.state());
    }

    // the input thread disables raw mode too, but main can exit first — do it again, idempotent
    let _ = crossterm::terminal::disable_raw_mode();
    if let Err(err) = state::save(&state_path, &engine.into_state()) {
        eprintln!("foltv: cannot save state: {err:#}");
    }
    mpv.kill();
    Ok(())
}
