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
                    .table(GameCompatibility::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(GameCompatibility::AddonId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(GameCompatibility::Id).integer().not_null())
                    .col(
                        ColumnDef::new(GameCompatibility::Version)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(GameCompatibility::Name).string().not_null())
                    .primary_key(
                        Index::create()
                            .col(GameCompatibility::AddonId)
                            .col(GameCompatibility::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_addon_game_compat")
                            .from(GameCompatibility::Table, GameCompatibility::AddonId)
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
            .drop_table(Table::drop().table(GameCompatibility::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum GameCompatibility {
    Table,
    AddonId,
    Id,
    Version,
    Name,
}
