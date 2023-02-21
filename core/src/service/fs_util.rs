use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use entity::addon_dir as AddonDir;
use regex::Regex;
use snafu::ResultExt;
use walkdir::WalkDir;

use crate::{
    addons::{Addon, AddonList},
    error::{self, Result},
};

fn extract_dependency(dep: &str) -> Option<String> {
    let re = Regex::new(r"^(.+?)(([<=>]+)(.*))?$").unwrap();
    re.captures(dep).map(|captures| captures[1].to_owned())
}

pub fn fs_get_addon(addon_dir: &PathBuf, name: &str) -> Result<Option<Addon>> {
    let addon_list = fs_get_addons(addon_dir)?;
    let found = addon_list.addons.into_iter().find(|x| x.name == name);
    Ok(found)
}

pub fn fs_get_addons(addon_dir: &PathBuf) -> Result<AddonList> {
    // TODO: move to fs_util, tak addon_dir as param
    let mut addon_list = AddonList {
        addons: vec![],
        errors: vec![],
    };

    // Ok(fs::metadata(addon_dir));

    fs::metadata(&addon_dir).context(error::AddonDirMetadataSnafu { dir: &addon_dir })?;

    for entry in WalkDir::new(addon_dir) {
        let entry_dir = entry.unwrap();
        let file_path = entry_dir.path();

        let file_name = entry_dir.file_name();
        let parent_dir_name = file_path.parent().and_then(|f| f.file_name());

        match parent_dir_name {
            None => continue,
            Some(parent_dir_name) => {
                let mut name = parent_dir_name.to_os_string();
                name.push(".txt");
                if name != file_name {
                    continue;
                }
            }
        }

        let addon_dir = file_path.parent().unwrap();

        match fs_read_addon(addon_dir) {
            Ok(addon) => addon_list.addons.push(addon),
            Err(err) => println!("{err}"), //addon_list.errors.push(err),
        }
    }

    Ok(addon_list)
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

    let file = fs_open_addon_metadata_file(path, addon_name)?;
    let re = Regex::new(r"## (.*): (.*)").unwrap();

    let mut addon = Addon {
        name: addon_name.to_owned(),
        depends_on: vec![],
    };

    let reader = BufReader::new(file);
    for line in reader.lines().flatten() {
        if line.starts_with("## DependsOn:") {
            let depends_on = match re.captures(&line) {
                Some(ref captures) => captures[2]
                    .split(' ')
                    .map(|s| s.to_owned())
                    .into_iter()
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
