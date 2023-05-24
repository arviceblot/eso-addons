use crate::m20230208_165547_add_categories::AddonDetail;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AddonDetail::Table)
                    .add_column(ColumnDef::new(Alias::new("version")).string())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AddonDetail::Table)
                    .drop_column(Alias::new("version"))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
