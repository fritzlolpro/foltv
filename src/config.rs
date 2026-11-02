use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Channel {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub channels: Vec<Channel>,
}

pub fn load(path: &Path) -> Result<Config> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read config {}", path.display()))?;
    let config: Config =
        toml::from_str(&text).with_context(|| format!("cannot parse config {}", path.display()))?;
    if config.channels.is_empty() {
        return Err(anyhow!("no channels configured"));
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parses_channels() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("foltv.toml");
        std::fs::write(
            &cfg_path,
            r#"
[[channels]]
name = "anime"
path = "D:/video/anime"

[[channels]]
name = "movies"
path = "E:/video/movies"
"#,
        )
        .unwrap();

        let cfg = load(&cfg_path).unwrap();

        assert_eq!(cfg.channels.len(), 2);
        assert_eq!(cfg.channels[0].name, "anime");
        assert_eq!(cfg.channels[0].path, PathBuf::from("D:/video/anime"));
        assert_eq!(cfg.channels[1].name, "movies");
        assert_eq!(cfg.channels[1].path, PathBuf::from("E:/video/movies"));
    }

    #[test]
    fn test_empty_channels_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("foltv.toml");
        std::fs::write(&cfg_path, "channels = []\n").unwrap();

        assert!(load(&cfg_path).is_err());
    }

    #[test]
    fn test_missing_file_is_error() {
        let dir = tempfile::tempdir().unwrap();

        assert!(load(&dir.path().join("nope.toml")).is_err());
    }
}
