use std::collections::{HashMap, HashSet};

use addons::Addon;
use config::AddonEntry;

pub mod addons;
pub mod api;
pub mod config;
pub mod error;
pub mod service;

pub fn get_missing_dependencies(installed: &[Addon]) -> impl Iterator<Item = String> {
    let addon_map: HashSet<_> = installed.iter().map(|a| a.name.clone()).collect();

    let missing: HashSet<_> = installed
        .iter()
        .flat_map(|a| a.depends_on.iter())
        .filter(|dep| !addon_map.contains(*dep))
        .map(ToOwned::to_owned)
        .collect();

    missing.into_iter()
}

pub fn get_unmanaged_addons<'a, I>(desired: &[AddonEntry], installed: I) -> Vec<&'a Addon>
where
    I: Iterator<Item = &'a Addon>,
{
    let desired_map: HashSet<_> = desired.iter().map(|a| &a.name).collect();

    installed
        .filter(|addon| !desired_map.contains(&addon.name))
        .collect()
}

pub fn get_unused_dependencies(installed: &[Addon], desired: &[AddonEntry]) -> Vec<String> {
    let mut dep_graph: HashMap<String, HashSet<String>> = HashMap::new();

    for addon in installed {
        dep_graph.entry(addon.name.clone()).or_default();

        for dependency in &addon.depends_on {
            dep_graph
                .entry(dependency.clone())
                .or_default()
                .insert(addon.name.clone());
        }
    }

    let mut unused_addons = vec![];

    for (addon, dependency_for) in &dep_graph {
        if dependency_for.is_empty() {
            let addon_config = desired.iter().find(|x| x.name == *addon);
            let unused = addon_config.map(|x| x.dependency).unwrap_or(true);

            if unused {
                unused_addons.push(addon.clone());
            }
        }
    }

    unused_addons
}
