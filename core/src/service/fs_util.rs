use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::LazyLock,
};

use entity::addon_dir as AddonDir;
use eso_addon_manifest::AddonManifestParser;
use regex::Regex;
use snafu::ResultExt;
use tracing::log::warn;

use crate::{
    addons::Addon,
    error::{self, Result},
};

static DEPENDENCY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+?)(([<=>]+)(.*))?$").unwrap());
static METADATA_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"## (.*): (.*)").unwrap());

/// Replace a substring case-insensitively, preserving surrounding separator chars.
pub fn case_insensitive_replace(s: &str, target: &str, replacement: &str) -> String {
    let lower = s.to_lowercase();
    let mut result = String::with_capacity(s.len());
    let mut last = 0;

    for m in lower.match_indices(target) {
        let start = m.0;
        let end = start + target.len();
        result.push_str(&s[last..start]);
        result.push_str(replacement);
        last = end;
    }
    result.push_str(&s[last..]);
    result
}

/// Suggest a PTS path based on a Live path (or vice-versa). Case-insensitive.
/// Returns `None` if the path doesn't contain "Live" or "PTS" segment.
pub fn suggest_alternate_game_path(path: &Path) -> Option<String> {
    let path_str = path.to_string_lossy().to_string();
    let lower = path_str.to_lowercase();

    if lower.contains("/pts/") || lower.contains("\\pts\\") {
        // Already a PTS path, suggest Live instead
        return Some(case_insensitive_replace(&path_str, "pts", "live"));
    }
    if lower.contains("/live/") || lower.contains("\\live\\") {
        return Some(case_insensitive_replace(&path_str, "live", "pts"));
    }
    None
}

/// Detect a human-readable label for an addon folder based on its path.
/// Returns "Live", "PTS", a capitalized basename, or "Custom".
pub fn detect_folder_label(path: &Path) -> String {
    let path_str = path.to_string_lossy().to_string();
    let lower = path_str.to_lowercase();

    if lower.contains("pts") {
        return "PTS".to_string();
    }
    if lower.contains("live") {
        return "Live".to_string();
    }

    // Fallback to directory basename, or "Custom"
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| {
            // Capitalize first letter
            let mut c = n.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().chain(c).collect(),
            }
        })
        .unwrap_or_else(|| "Custom".to_string())
}

/// Try to parse a manifest file at the given path.
/// Returns `None` if the file doesn't exist or can't be parsed.
pub fn parse_manifest_file(path: &Path) -> Option<eso_addon_manifest::AddonManifest> {
    if !path.exists() {
        return None;
    }
    match AddonManifestParser::default().parse(path.to_str()?, None) {
        Ok(m) => Some(m),
        Err(e) => {
            warn!("Failed to parse manifest {:?}: {}", path, e);
            None
        }
    }
}

/// Like [`parse_manifest_file`] but tries `.addon` first, then `.txt` variants.
/// ESO prefers `.addon` manifest files over `.txt` when both exist.
pub fn parse_full_manifest(
    dir_path: &Path,
    dir_name: &str,
) -> Option<eso_addon_manifest::AddonManifest> {
    // .addon takes precedence over .txt
    let addon_path = dir_path.join(format!("{dir_name}.addon"));
    let addon_lower = dir_path.join(format!("{}.addon", dir_name.to_lowercase()));
    let txt_path = dir_path.join(format!("{dir_name}.txt"));
    let txt_lower = dir_path.join(format!("{}.txt", dir_name.to_lowercase()));
    parse_manifest_file(&addon_path)
        .or_else(|| parse_manifest_file(&addon_lower))
        .or_else(|| parse_manifest_file(&txt_path))
        .or_else(|| parse_manifest_file(&txt_lower))
}

fn extract_dependency(dep: &str) -> Option<String> {
    DEPENDENCY_RE
        .captures(dep)
        .map(|captures| captures[1].to_owned())
}

fn fs_open_addon_metadata_file(path: &Path, addon_name: &str) -> Result<File> {
    let mut filepath = path.to_owned();
    let mut filepath_lowercase = path.to_owned();

    let filename = PathBuf::from(format!("{addon_name}.txt"));
    let filename_lowercase = PathBuf::from(format!("{}.txt", addon_name.to_lowercase()));

    filepath.push(filename);
    filepath_lowercase.push(filename_lowercase);

    if filepath.exists() {
        Ok(File::open(&filepath).context(error::AddonMetadataFileSnafu { path: filepath })?)
    } else if filepath_lowercase.exists() {
        Ok(File::open(&filepath_lowercase)
            .context(error::AddonMetadataFileSnafu { path: filepath })?)
    } else {
        error::AddonMetadataFileMissingSnafu { addon: addon_name }.fail()
    }
}

pub fn fs_has_addon_manifest(path: &Path) -> Result<bool> {
    for child in fs::read_dir(path).context(error::AddonDirReadSnafu { dir: path })? {
        let child = child.context(error::AddonDirReadSnafu { dir: path })?;
        let child_path = child.path();
        let extension = child_path.extension().and_then(|ext| ext.to_str());
        let is_file = child
            .file_type()
            .context(error::AddonDirMetadataSnafu { dir: &child_path })?
            .is_file();

        if extension == Some("addon") && is_file {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn fs_read_addon(path: &Path) -> Result<Addon> {
    let addon_name = path.file_name().unwrap().to_str().unwrap();
    let mut addon = Addon {
        name: addon_name.to_owned(),
        depends_on: vec![],
    };

    // Not all addons have a Metadata file but are still valid addons, such as HarvestMapData
    let file = fs_open_addon_metadata_file(path, addon_name);
    match file {
        Ok(_) => {}
        Err(_) => return Ok(addon),
    }
    let addon_file = file.unwrap();

    let re = &METADATA_RE;

    let reader = BufReader::new(addon_file);
    for line in reader.lines().map_while(Result::ok) {
        if line.starts_with("## DependsOn:") {
            let depends_on = match re.captures(&line) {
                Some(ref captures) => captures[2]
                    .split(' ')
                    .map(|s| s.to_owned())
                    .filter_map(|s| extract_dependency(&s))
                    .collect(),
                None => vec![],
            };

            addon.depends_on = depends_on;
        }
    }

    Ok(addon)
}

pub fn fs_delete_addon(addon_path: &PathBuf, addon_dirs: &[AddonDir::Model]) -> Result<()> {
    for dir in addon_dirs.iter() {
        let full_path = Path::new(&addon_path).join(&dir.dir);
        fs::remove_dir_all(&full_path).context(error::AddonDeleteSnafu { dir: full_path })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_folder_label_live() {
        assert_eq!(
            detect_folder_label(Path::new(
                "/home/user/My Games/The Elder Scrolls Online/live/AddOns"
            )),
            "Live"
        );
        // Case insensitive
        assert_eq!(
            detect_folder_label(Path::new("/games/ESO/LIVE/AddOns")),
            "Live"
        );
        // Windows-style
        assert_eq!(
            detect_folder_label(Path::new("C:\\Users\\eso\\My Games\\ESO\\live\\AddOns")),
            "Live"
        );
    }

    #[test]
    fn test_detect_folder_label_pts() {
        assert_eq!(
            detect_folder_label(Path::new(
                "/home/user/My Games/The Elder Scrolls Online/pts/AddOns"
            )),
            "PTS"
        );
        // PTS takes precedence over Live
        assert_eq!(
            detect_folder_label(Path::new("/games/ESO/live-pts-mixed/AddOns")),
            "PTS"
        );
        // Case insensitive
        assert_eq!(
            detect_folder_label(Path::new("/games/ESO/PTS/AddOns")),
            "PTS"
        );
    }

    #[test]
    fn test_detect_folder_label_basename_fallback() {
        assert_eq!(
            detect_folder_label(Path::new("/games/eso/custom_folder")),
            "Custom_folder"
        );
        assert_eq!(
            detect_folder_label(Path::new("/path/to/my_addons")),
            "My_addons"
        );
    }

    #[test]
    fn test_detect_folder_label_custom_fallback() {
        // Path with no file_name component
        assert_eq!(detect_folder_label(Path::new("/")), "Custom");
    }

    #[test]
    fn test_suggest_alternate_game_path_live_to_pts() {
        let live = Path::new("/home/user/My Games/The Elder Scrolls Online/live/AddOns");
        let suggested = suggest_alternate_game_path(live);
        assert_eq!(
            suggested,
            Some("/home/user/My Games/The Elder Scrolls Online/pts/AddOns".to_string())
        );
    }

    #[test]
    fn test_suggest_alternate_game_path_pts_to_live() {
        let pts = Path::new("/home/user/My Games/The Elder Scrolls Online/pts/AddOns");
        let suggested = suggest_alternate_game_path(pts);
        assert_eq!(
            suggested,
            Some("/home/user/My Games/The Elder Scrolls Online/live/AddOns".to_string())
        );
    }

    #[test]
    fn test_suggest_alternate_game_path_case_insensitive() {
        let live = Path::new("/games/ESO/LIVE/AddOns");
        let suggested = suggest_alternate_game_path(live);
        assert!(suggested.is_some());
        assert!(suggested.unwrap().to_lowercase().contains("pts"));
    }

    #[test]
    fn test_suggest_alternate_game_path_no_match() {
        let custom = Path::new("/games/ESO/unknown/AddOns");
        assert!(suggest_alternate_game_path(custom).is_none());
    }
}
