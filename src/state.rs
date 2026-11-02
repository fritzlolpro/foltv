use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ChannelState {
    pub file: PathBuf,
    pub position: f64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct State {
    pub last_channel: Option<String>,
    pub volume: f64,
    pub mute: bool,
    pub channels: HashMap<String, ChannelState>,
}

pub fn default_state() -> State {
    State {
        last_channel: None,
        volume: 100.0,
        mute: false,
        channels: HashMap::new(),
    }
}

pub fn load(path: &Path) -> State {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(default_state)
}

pub fn save(path: &Path, state: &State) -> Result<()> {
    let text = serde_json::to_string_pretty(state)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("foltv-state.json");
        let state = State {
            last_channel: Some("anime".into()),
            volume: 42.5,
            mute: true,
            channels: HashMap::from([(
                "anime".to_string(),
                ChannelState { file: PathBuf::from("D:/video/anime/e2.mkv"), position: 123.4 },
            )]),
        };

        save(&path, &state).unwrap();
        let loaded = load(&path);

        assert_eq!(loaded, state);
    }

    #[test]
    fn test_missing_state_is_default() {
        let dir = tempfile::tempdir().unwrap();

        let state = load(&dir.path().join("nope.json"));

        assert_eq!(state.volume, 100.0);
        assert_eq!(state.last_channel, None);
        assert!(!state.mute);
        assert!(state.channels.is_empty());
    }

    #[test]
    fn test_corrupt_state_loads_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("foltv-state.json");
        std::fs::write(&path, "not json{{{").unwrap();

        let state = load(&path);

        assert_eq!(state.volume, 100.0);
        assert_eq!(state.last_channel, None);
        assert!(!state.mute);
        assert!(state.channels.is_empty());
    }

    #[test]
    fn test_save_leaves_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("foltv-state.json");

        save(&path, &default_state()).unwrap();

        let tmps: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(tmps.is_empty());
    }
}
