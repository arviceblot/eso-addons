use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // addon_image: composite PK (addon_id, index) already exists;
        // add a single-column index on addon_id for efficient IN(...) lookups.
        manager
            .create_index(
                Index::create()
                    .name("idx_addon_image_addon_id")
                    .table(AddonImage::Table)
                    .col(AddonImage::AddonId)
                    .to_owned(),
            )
            .await?;

        // game_compatibility: composite PK (addon_id, id) already exists.
        manager
            .create_index(
                Index::create()
                    .name("idx_game_compatibility_addon_id")
                    .table(GameCompatibility::Table)
                    .col(GameCompatibility::AddonId)
                    .to_owned(),
            )
            .await?;

        // addon_dir: composite PK (addon_id, dir) already exists.
        manager
            .create_index(
                Index::create()
                    .name("idx_addon_dir_addon_id")
                    .table(AddonDir::Table)
                    .col(AddonDir::AddonId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_addon_image_addon_id")
                    .table(AddonImage::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_game_compatibility_addon_id")
                    .table(GameCompatibility::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_addon_dir_addon_id")
                    .table(AddonDir::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum AddonImage {
    Table,
    AddonId,
}

#[derive(Iden)]
enum GameCompatibility {
    Table,
    AddonId,
}

#[derive(Iden)]
enum AddonDir {
    Table,
    AddonId,
}
