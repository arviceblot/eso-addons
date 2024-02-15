use sea_orm_migration::prelude::*;

use crate::m20230208_165547_add_categories::Category;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Change main category name from "All" to "Any"
        let update = Query::update()
            .table(Category::Table)
            .values([(Category::Title, "Any".into())])
            .and_where(Expr::col(Category::Id).eq(1))
            .to_owned();
        manager.exec_stmt(update).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // revert change
        let update = Query::update()
            .table(Category::Table)
            .values([(Category::Title, "All".into())])
            .and_where(Expr::col(Category::Id).eq(1))
            .to_owned();
        manager.exec_stmt(update).await?;

        Ok(())
    }
}
