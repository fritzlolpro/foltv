use rand::seq::IndexedRandom;
use std::path::{Path, PathBuf};

const VIDEO_EXTENSIONS: [&str; 8] = ["mp4", "mkv", "avi", "webm", "mov", "m4v", "ts", "flv"];

pub fn scan_videos(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path
                .extension()
                .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
            {
                out.push(path);
            }
        }
    }
    let mut out = vec![];
    walk(root, &mut out);
    out
}

pub fn pick_random(videos: &[PathBuf], exclude: Option<&Path>) -> Option<PathBuf> {
    if videos.is_empty() {
        return None;
    }
    let mut rng = rand::rng();
    let pool: Vec<&PathBuf> = videos
        .iter()
        .filter(|v| exclude.map_or(true, |ex| v.as_path() != ex))
        .collect();
    if pool.is_empty() {
        videos.choose(&mut rng).cloned()
    } else {
        pool.choose(&mut rng).cloned().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    fn make_tree() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.mp4"), b"x").unwrap();
        std::fs::create_dir(root.path().join("sub")).unwrap();
        std::fs::write(root.path().join("sub").join("b.MKV"), b"x").unwrap();
        std::fs::create_dir_all(root.path().join("sub").join("nested")).unwrap();
        std::fs::write(root.path().join("sub").join("nested").join("c.webm"), b"x").unwrap();
        std::fs::write(root.path().join("note.txt"), b"x").unwrap();
        std::fs::write(root.path().join("cover.jpg"), b"x").unwrap();
        root
    }

    #[test]
    fn test_scan_is_recursive_and_filters() {
        let root = make_tree();

        let scanned: BTreeSet<PathBuf> = scan_videos(root.path()).into_iter().collect();

        let expected: BTreeSet<PathBuf> = [
            root.path().join("a.mp4"),
            root.path().join("sub").join("b.MKV"),
            root.path().join("sub").join("nested").join("c.webm"),
        ]
        .into_iter()
        .collect();
        assert_eq!(scanned, expected);
    }

    #[test]
    fn test_scan_missing_root_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");

        assert!(scan_videos(&missing).is_empty());
    }

    #[test]
    fn test_pick_excludes_previous() {
        let root = make_tree();
        let videos = scan_videos(root.path());
        let a = root.path().join("a.mp4");

        let mut seen = BTreeSet::new();
        for _ in 0..100 {
            let picked = pick_random(&videos, Some(&a)).unwrap();
            assert_ne!(picked, a);
            assert!(videos.contains(&picked));
            seen.insert(picked);
        }
        // both other files show up eventually
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn test_single_file_excluded_still_returns_it() {
        let x = PathBuf::from("only.mkv");
        let videos = vec![x.clone()];

        assert_eq!(pick_random(&videos, Some(&x)), Some(x));
    }

    #[test]
    fn test_pick_empty_is_none() {
        let empty: Vec<PathBuf> = vec![];
        let some = PathBuf::from("x.mp4");

        assert_eq!(pick_random(&empty, None), None);
        assert_eq!(pick_random(&empty, Some(&some)), None);
    }
}
