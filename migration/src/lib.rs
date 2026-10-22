pub use sea_orm_migration::prelude::*;

mod m20220101_000001_create_table;
mod m20230208_165547_add_categories;
mod m20230302_100852_addon_detail_version;
mod m20230519_153409_manual_deps;
mod m20240212_164739_update_0_category;
mod m20240214_174800_add_game_version;
mod m20240219_192429_add_addon_images;
mod m20261010_000001_multiple_sources;
mod m20261013_000001_child_table_indexes;
mod m20261015_000001_multiple_addon_folders;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20220101_000001_create_table::Migration),
            Box::new(m20230208_165547_add_categories::Migration),
            Box::new(m20230302_100852_addon_detail_version::Migration),
            Box::new(m20230519_153409_manual_deps::Migration),
            Box::new(m20240212_164739_update_0_category::Migration),
            Box::new(m20240214_174800_add_game_version::Migration),
            Box::new(m20240219_192429_add_addon_images::Migration),
            Box::new(m20261010_000001_multiple_sources::Migration),
            Box::new(m20261013_000001_child_table_indexes::Migration),
            Box::new(m20261015_000001_multiple_addon_folders::Migration),
        ]
    }
}
