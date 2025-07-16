use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use entity::addon_dir as AddonDir;
use regex::Regex;
use snafu::ResultExt;

use crate::{
    addons::Addon,
    error::{self, Result},
};

fn extract_dependency(dep: &str) -> Option<String> {
    let re = Regex::new(r"^(.+?)(([<=>]+)(.*))?$").unwrap();
    re.captures(dep).map(|captures| captures[1].to_owned())
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

    let re = Regex::new(r"## (.*): (.*)").unwrap();

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
