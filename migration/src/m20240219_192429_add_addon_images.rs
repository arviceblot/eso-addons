use crate::m20220101_000001_create_table::Addon;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AddonImage::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(AddonImage::AddonId).integer().not_null())
                    .col(ColumnDef::new(AddonImage::Index).integer().not_null())
                    .col(ColumnDef::new(AddonImage::Thumbnail).string().not_null())
                    .col(ColumnDef::new(AddonImage::Image).string().not_null())
                    .primary_key(
                        Index::create()
                            .col(AddonImage::AddonId)
                            .col(AddonImage::Index),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_addon_image")
                            .from(AddonImage::Table, AddonImage::AddonId)
                            .to(Addon::Table, Addon::Id)
                            .on_delete(ForeignKeyAction::NoAction)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AddonImage::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum AddonImage {
    Table,
    AddonId,
    Index,
    Thumbnail,
    Image,
}
