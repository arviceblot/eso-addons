use crate::error::Error;
use regex::Regex;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Addon {
    pub name: String,
    pub depends_on: Vec<String>,
}

pub struct AddonList {
    pub addons: Vec<Addon>,
    pub errors: Vec<Error>,
}

pub fn get_download_url(addon_url: &str) -> Option<String> {
    let fns: Vec<fn(&str) -> Option<String>> = vec![
        |url: &str| {
            let re = Regex::new(r"^https://.*esoui\.com/downloads/info(\d+)-(.+)$").unwrap();
            re.captures(url).map(|captures| captures[1].to_owned())
        },
        |url: &str| {
            let re =
                Regex::new(r"^https://.+esoui\.com/downloads/fileinfo\.php\?id=(\d+)$").unwrap();
            re.captures(url).map(|captures| captures[1].to_owned())
        },
    ];

    for f in fns {
        let url = f(addon_url);
        if let Some(id) = url {
            return Some(format!("https://www.esoui.com/downloads/download{}", id));
        }
    }

    None
}

pub fn get_root_dir(path: &Path) -> PathBuf {
    match path.parent() {
        None => path.to_owned(),
        Some(parent) => match parent.to_str().unwrap() {
            "" => path.to_owned(),
            &_ => get_root_dir(parent),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_download_link() {
        let tests: Vec<(&str, Option<String>)> = vec![
            (
                "https://www.esoui.com/downloads/info1360-CombatMetrics",
                Some("https://www.esoui.com/downloads/download1360".to_string()),
            ),
            (
                "https://www.esoui.com/downloads/info1360-CombatMetrics.html",
                Some("https://www.esoui.com/downloads/download1360".to_string()),
            ),
            (
                "https://www.esoui.com/downloads/fileinfo.php?id=2817",
                Some("https://www.esoui.com/downloads/download2817".to_string()),
            ),
        ];

        for test in tests {
            let url = get_download_url(test.0);
            assert!(url == test.1, "Got value: {:?}", url);
        }
    }
}
