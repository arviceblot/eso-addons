//! SeaORM Entity. Generated by sea-orm-codegen 0.9.3

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "addon")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i32,
    pub category_id: String,
    pub version: String,
    pub date: u64,
    pub name: String,
    pub author_name: Option<String>,
    pub file_info_url: Option<String>,
    pub download_total: Option<String>,
    pub download_monthly: Option<String>,
    pub favorite_total: Option<String>,
    pub md5: Option<String>,
    pub file_name: Option<String>,
    pub download: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::installed_addon::Entity")]
    InstalledAddon,
    #[sea_orm(has_many = "super::addon_dir::Entity")]
    AddonDir,
}

impl Related<super::installed_addon::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::InstalledAddon.def()
    }
}

impl Related<super::addon_dir::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AddonDir.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
