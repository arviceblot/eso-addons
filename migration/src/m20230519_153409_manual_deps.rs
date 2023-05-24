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
                    .table(ManualDependency::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ManualDependency::AddonDir)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ManualDependency::SatisfiedBy).integer())
                    .col(ColumnDef::new(ManualDependency::Ignore).boolean())
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_manual_dependency_addon")
                            .from(ManualDependency::Table, ManualDependency::SatisfiedBy)
                            .to(Addon::Table, Addon::Id)
                            .on_delete(ForeignKeyAction::NoAction)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .to_owned(),
            )
            .await
        // TODO: add check constraint on (SatisfiedBy is not null OR Ignore == 1)
        // when sea_query 0.29 available
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ManualDependency::Table).to_owned())
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
enum ManualDependency {
    Table,
    AddonDir,
    SatisfiedBy,
    Ignore,
}
