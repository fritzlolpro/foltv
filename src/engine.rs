use crate::config::{Channel, Config};
use crate::library::{pick_random, scan_videos};
use crate::state::{ChannelState, State};
use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(PartialEq, Debug)]
pub enum Command {
    Next,
    ChannelUp,
    ChannelDown,
    PauseToggle,
    VolumeUp,
    VolumeDown,
    MuteToggle,
    Quit,
}

pub enum PlayerEvent {
    FileEnded,
}

pub trait Player {
    fn load_file(&mut self, path: &Path, start: Option<f64>) -> Result<()>;
    fn position(&mut self) -> Result<f64>;
    fn set_paused(&mut self, paused: bool) -> Result<()>;
    fn set_volume(&mut self, volume: f64) -> Result<()>;
    fn set_mute(&mut self, mute: bool) -> Result<()>;
}

pub struct Engine {
    config: Config,
    state: State,
    current: Option<PathBuf>,
    channel_idx: usize,
    paused: bool,
}

impl Engine {
    pub fn new(config: Config, state: State) -> Self {
        let channel_idx = state
            .last_channel
            .as_ref()
            .and_then(|name| config.channels.iter().position(|c| &c.name == name))
            .unwrap_or(0);
        Engine { config, state, current: None, channel_idx, paused: false }
    }

    fn channel(&self) -> &Channel {
        &self.config.channels[self.channel_idx]
    }

    fn enter_channel(&mut self, idx: usize, player: &mut dyn Player) -> Result<()> {
        self.channel_idx = idx;
        self.current = None;
        let name = self.config.channels[idx].name.clone();
        let folder = self.config.channels[idx].path.clone();
        self.state.last_channel = Some(name.clone());
        if let Some(entry) = self.state.channels.get(&name) {
            if entry.file.exists() {
                player.load_file(&entry.file, Some(entry.position))?;
                self.current = Some(entry.file.clone());
                return Ok(());
            }
        }
        if let Some(pick) = pick_random(&scan_videos(&folder), None) {
            player.load_file(&pick, None)?;
            self.current = Some(pick);
        }
        Ok(())
    }

    pub fn start(&mut self, player: &mut dyn Player) -> Result<()> {
        let idx = self.channel_idx;
        player.set_volume(self.state.volume)?;
        if self.state.mute {
            player.set_mute(true)?;
        }
        self.enter_channel(idx, player)
    }

    fn save_current(&mut self, player: &mut dyn Player) {
        let Some(file) = self.current.clone() else { return };
        let Ok(position) = player.position() else { return };
        let name = self.channel().name.clone();
        self.state.channels.insert(name, ChannelState { file, position });
    }

    fn advance(&mut self, player: &mut dyn Player) {
        let name = self.channel().name.clone();
        self.state.channels.remove(&name);
        let exclude = self.current.take();
        let folder = self.channel().path.clone();
        if let Some(pick) = pick_random(&scan_videos(&folder), exclude.as_deref()) {
            if player.load_file(&pick, None).is_ok() {
                self.current = Some(pick);
            }
        }
    }

    fn switch_channel(&mut self, up: bool, player: &mut dyn Player) {
        let len = self.config.channels.len();
        let new_idx = if up {
            (self.channel_idx + 1) % len
        } else {
            (self.channel_idx + len - 1) % len
        };
        if new_idx == self.channel_idx {
            return; // single channel: nothing to switch to
        }
        self.save_current(player);
        let _ = self.enter_channel(new_idx, player);
    }

    pub fn on_player_event(&mut self, event: PlayerEvent, player: &mut dyn Player) {
        match event {
            PlayerEvent::FileEnded => self.advance(player),
        }
    }

    pub fn on_command(&mut self, cmd: Command, player: &mut dyn Player) -> bool {
        match cmd {
            Command::Next => self.advance(player),
            Command::ChannelUp => self.switch_channel(true, player),
            Command::ChannelDown => self.switch_channel(false, player),
            Command::PauseToggle => {
                self.paused = !self.paused;
                let _ = player.set_paused(self.paused);
            }
            Command::VolumeUp => {
                self.state.volume = (self.state.volume + 5.0).min(100.0);
                let _ = player.set_volume(self.state.volume);
            }
            Command::VolumeDown => {
                self.state.volume = (self.state.volume - 5.0).max(0.0);
                let _ = player.set_volume(self.state.volume);
            }
            Command::MuteToggle => {
                self.state.mute = !self.state.mute;
                let _ = player.set_mute(self.state.mute);
            }
            Command::Quit => {
                self.save_current(player);
                return false;
            }
        }
        true
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn into_state(self) -> State {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Channel, Config};
    use crate::state::{ChannelState, State, default_state};
    use anyhow::bail;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    struct MockPlayer {
        loads: Vec<(PathBuf, Option<f64>)>,
        position: f64,
        position_fails: bool,
        volumes: Vec<f64>,
        mutes: Vec<bool>,
        pauses: Vec<bool>,
    }

    impl MockPlayer {
        fn new(position: f64) -> Self {
            MockPlayer {
                loads: vec![],
                position,
                position_fails: false,
                volumes: vec![],
                mutes: vec![],
                pauses: vec![],
            }
        }
    }

    impl Player for MockPlayer {
        fn load_file(&mut self, path: &Path, start: Option<f64>) -> anyhow::Result<()> {
            self.loads.push((path.to_path_buf(), start));
            Ok(())
        }
        fn position(&mut self) -> anyhow::Result<f64> {
            if self.position_fails {
                bail!("no file loaded");
            }
            Ok(self.position)
        }
        fn set_paused(&mut self, paused: bool) -> anyhow::Result<()> {
            self.pauses.push(paused);
            Ok(())
        }
        fn set_volume(&mut self, volume: f64) -> anyhow::Result<()> {
            self.volumes.push(volume);
            Ok(())
        }
        fn set_mute(&mut self, mute: bool) -> anyhow::Result<()> {
            self.mutes.push(mute);
            Ok(())
        }
    }

    fn setup() -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        for f in ["e1.mkv", "e2.mkv", "e3.mkv"] {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        let empty = dir.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        let config = Config {
            channels: vec![
                Channel { name: "anime".into(), path: dir.path().to_path_buf() },
                Channel { name: "empty".into(), path: empty },
            ],
        };
        (dir, config)
    }

    fn resumed_state(dir: &Path, file: &str, pos: f64) -> State {
        State {
            last_channel: Some("anime".into()),
            volume: 42.5,
            mute: false,
            channels: HashMap::from([(
                "anime".to_string(),
                ChannelState { file: dir.join(file), position: pos },
            )]),
        }
    }

    #[test]
    fn start_resumes_last_channel() {
        let (dir, config) = setup();
        let mut engine = Engine::new(config, resumed_state(dir.path(), "e2.mkv", 61.0));
        let mut player = MockPlayer::new(0.0);

        engine.start(&mut player).unwrap();

        assert_eq!(player.loads, vec![(dir.path().join("e2.mkv"), Some(61.0))]);
        assert_eq!(player.volumes, vec![42.5]);
        assert!(player.mutes.is_empty());
    }

    #[test]
    fn start_resumes_missing_file_falls_back_to_random() {
        let (dir, config) = setup();
        let mut engine = Engine::new(config, resumed_state(dir.path(), "gone.mkv", 61.0));
        let mut player = MockPlayer::new(0.0);

        engine.start(&mut player).unwrap();

        assert_eq!(player.loads.len(), 1);
        let (loaded, start) = &player.loads[0];
        assert_eq!(*start, None);
        assert!(crate::library::scan_videos(dir.path()).contains(loaded));
    }

    #[test]
    fn start_no_state_picks_random() {
        let (dir, config) = setup();
        let mut engine = Engine::new(config, default_state());
        let mut player = MockPlayer::new(0.0);

        engine.start(&mut player).unwrap();

        assert_eq!(player.loads.len(), 1);
        let (loaded, start) = &player.loads[0];
        assert_eq!(*start, None);
        assert!(crate::library::scan_videos(dir.path()).contains(loaded));
        assert_eq!(engine.state().last_channel.as_deref(), Some("anime"));
    }

    #[test]
    fn file_ended_loads_next_and_clears_entry() {
        let (dir, config) = setup();
        let mut engine = Engine::new(config, resumed_state(dir.path(), "e2.mkv", 61.0));
        let mut player = MockPlayer::new(0.0);
        engine.start(&mut player).unwrap();

        engine.on_player_event(PlayerEvent::FileEnded, &mut player);

        assert_eq!(player.loads.len(), 2);
        let (loaded, start) = &player.loads[1];
        assert_eq!(*start, None);
        assert_ne!(loaded, &dir.path().join("e2.mkv"));
        assert!(engine.state().channels.get("anime").is_none());
    }

    #[test]
    fn next_discards_position() {
        let (dir, config) = setup();
        let mut engine = Engine::new(config, resumed_state(dir.path(), "e1.mkv", 10.0));
        let mut player = MockPlayer::new(500.0);
        engine.start(&mut player).unwrap();

        assert!(engine.on_command(Command::Next, &mut player));

        assert_eq!(player.loads.len(), 2);
        let (loaded, start) = &player.loads[1];
        assert_eq!(*start, None);
        assert_ne!(loaded, &dir.path().join("e1.mkv"));
        assert!(engine.state().channels.get("anime").is_none());
    }

    #[test]
    fn channel_switch_saves_and_restores() {
        let (dir, config) = setup();
        let mut engine = Engine::new(config, resumed_state(dir.path(), "e1.mkv", 61.0));
        let mut player = MockPlayer::new(77.7);
        engine.start(&mut player).unwrap();

        assert!(engine.on_command(Command::ChannelUp, &mut player));

        assert_eq!(
            engine.state().channels.get("anime"),
            Some(&ChannelState { file: dir.path().join("e1.mkv"), position: 77.7 })
        );
        assert_eq!(player.loads.len(), 1); // nothing loaded on the empty channel
        assert_eq!(engine.state().last_channel.as_deref(), Some("empty"));

        assert!(engine.on_command(Command::ChannelDown, &mut player));

        assert_eq!(player.loads.len(), 2);
        assert_eq!(player.loads[1], (dir.path().join("e1.mkv"), Some(77.7)));
        assert_eq!(engine.state().last_channel.as_deref(), Some("anime"));
    }

    #[test]
    fn single_channel_switch_is_noop() {
        let (dir, _config) = setup();
        let config = Config {
            channels: vec![Channel { name: "anime".into(), path: dir.path().to_path_buf() }],
        };
        let mut engine = Engine::new(config, resumed_state(dir.path(), "e1.mkv", 61.0));
        let mut player = MockPlayer::new(77.7);
        engine.start(&mut player).unwrap();
        let loads_before = player.loads.len();
        let volumes_before = player.volumes.len();

        assert!(engine.on_command(Command::ChannelUp, &mut player));
        assert!(engine.on_command(Command::ChannelDown, &mut player));

        assert_eq!(player.loads.len(), loads_before);
        assert_eq!(player.volumes.len(), volumes_before);
        // no save happened: the pre-existing resume entry is untouched (position still 61.0, not 77.7)
        assert_eq!(
            engine.state().channels.get("anime"),
            Some(&ChannelState { file: dir.path().join("e1.mkv"), position: 61.0 })
        );
    }

    #[test]
    fn volume_clamps_and_persists() {
        let (dir, config) = setup();
        let mut state = resumed_state(dir.path(), "e1.mkv", 61.0);
        state.volume = 98.0;
        let mut engine = Engine::new(config, state);
        let mut player = MockPlayer::new(0.0);
        engine.start(&mut player).unwrap();

        assert!(engine.on_command(Command::VolumeUp, &mut player));

        assert_eq!(player.volumes, vec![98.0, 100.0]);
        assert_eq!(engine.state().volume, 100.0);

        let (dir, config) = setup();
        let mut state = resumed_state(dir.path(), "e1.mkv", 61.0);
        state.volume = 10.0;
        let mut engine = Engine::new(config, state);
        let mut player = MockPlayer::new(0.0);
        engine.start(&mut player).unwrap();
        for _ in 0..5 {
            engine.on_command(Command::VolumeDown, &mut player);
        }

        assert_eq!(player.volumes.last(), Some(&0.0));
        assert_eq!(engine.state().volume, 0.0);
    }

    #[test]
    fn mute_toggles() {
        let (dir, config) = setup();
        let mut engine = Engine::new(config, resumed_state(dir.path(), "e1.mkv", 61.0));
        let mut player = MockPlayer::new(0.0);
        engine.start(&mut player).unwrap();

        engine.on_command(Command::MuteToggle, &mut player);
        engine.on_command(Command::MuteToggle, &mut player);

        assert_eq!(player.mutes, vec![true, false]);
        assert!(!engine.state().mute);
    }

    #[test]
    fn pause_toggles() {
        let (dir, config) = setup();
        let mut engine = Engine::new(config, resumed_state(dir.path(), "e1.mkv", 61.0));
        let mut player = MockPlayer::new(0.0);
        engine.start(&mut player).unwrap();

        engine.on_command(Command::PauseToggle, &mut player);
        engine.on_command(Command::PauseToggle, &mut player);

        assert_eq!(player.pauses, vec![true, false]);
    }

    #[test]
    fn quit_saves_current_position() {
        let (dir, config) = setup();
        let mut engine = Engine::new(config, resumed_state(dir.path(), "e1.mkv", 61.0));
        let mut player = MockPlayer::new(123.5);
        engine.start(&mut player).unwrap();

        assert!(!engine.on_command(Command::Quit, &mut player));

        assert_eq!(
            engine.into_state().channels.get("anime"),
            Some(&ChannelState { file: dir.path().join("e1.mkv"), position: 123.5 })
        );
    }

    #[test]
    fn idle_errors_are_swallowed() {
        // leaving a channel while position() fails: no entry, no panic
        let (dir, config) = setup();
        let mut engine = Engine::new(config, default_state());
        let mut player = MockPlayer::new(0.0);
        player.position_fails = true;
        engine.start(&mut player).unwrap();

        assert!(engine.on_command(Command::ChannelUp, &mut player));
        assert!(engine.state().channels.get("anime").is_none());

        // quitting while position() fails: no entry, no panic
        let (_dir, config) = setup();
        let mut engine = Engine::new(config, default_state());
        let mut player = MockPlayer::new(0.0);
        player.position_fails = true;
        engine.start(&mut player).unwrap();

        assert!(!engine.on_command(Command::Quit, &mut player));
        assert!(engine.into_state().channels.is_empty());
    }
}
