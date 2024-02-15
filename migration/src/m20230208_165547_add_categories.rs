use crate::m20220101_000001_create_table::Addon;
use sea_orm_migration::{
    prelude::*,
    sea_orm::{ConnectionTrait, DatabaseBackend, Statement},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // AddonDetail
        manager
            .create_table(
                Table::create()
                    .table(AddonDetail::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AddonDetail::Id)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AddonDetail::Description).string())
                    .col(ColumnDef::new(AddonDetail::ChangeLog).string())
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_addon_detail_addon")
                            .from(AddonDetail::Table, AddonDetail::Id)
                            .to(Addon::Table, Addon::Id)
                            .on_delete(ForeignKeyAction::NoAction)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .to_owned(),
            )
            .await?;

        // Category
        manager
            .create_table(
                Table::create()
                    .table(Category::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Category::Id)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Category::Title).string().not_null())
                    .col(ColumnDef::new(Category::Icon).string())
                    .col(ColumnDef::new(Category::FileCount).integer())
                    .to_owned(),
            )
            .await?;

        // CategoryParent
        manager
            .create_table(
                Table::create()
                    .table(CategoryParent::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(CategoryParent::Id).integer().not_null())
                    .col(
                        ColumnDef::new(CategoryParent::ParentId)
                            .integer()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(CategoryParent::Id)
                            .col(CategoryParent::ParentId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_category_parent_id")
                            .from(CategoryParent::Table, CategoryParent::Id)
                            .to(Category::Table, Category::Id)
                            .on_delete(ForeignKeyAction::NoAction)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_category_parent_parent_id")
                            .from(CategoryParent::Table, CategoryParent::ParentId)
                            .to(Category::Table, Category::Id)
                            .on_delete(ForeignKeyAction::NoAction)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .to_owned(),
            )
            .await?;

        // Move Addon data to tmp table to recreate new FKs
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            r#"CREATE TABLE addon_tmp AS SELECT * FROM addon;"#.to_owned(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            r#"PRAGMA foreign_keys = OFF;"#.to_owned(),
        ))
        .await?;

        // Drop original Addon table
        manager
            .drop_table(Table::drop().table(Addon::Table).to_owned())
            .await?;
        // Recreate Addon table with new FKs
        manager
            .create_table(
                Table::create()
                    .table(Addon::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Addon::Id).integer().not_null().primary_key())
                    .col(ColumnDef::new(Addon::CategoryId).string().not_null())
                    .col(ColumnDef::new(Addon::Version).string().not_null())
                    .col(ColumnDef::new(Addon::Date).string().not_null())
                    .col(ColumnDef::new(Addon::Name).string().not_null())
                    .col(ColumnDef::new(Addon::AuthorName).string())
                    .col(ColumnDef::new(Addon::FileInfoUrl).string())
                    .col(ColumnDef::new(Addon::DownloadTotal).string())
                    .col(ColumnDef::new(Addon::DownloadMonthly).string())
                    .col(ColumnDef::new(Addon::FavoriteTotal).string())
                    .col(ColumnDef::new(Addon::Md5).string())
                    .col(ColumnDef::new(Addon::FileName).string())
                    .col(ColumnDef::new(Addon::Download).string())
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_addon_category")
                            .from(Addon::Table, Addon::CategoryId)
                            .to(Category::Table, Category::Id)
                            .on_delete(ForeignKeyAction::NoAction)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .to_owned(),
            )
            .await?;

        // Move data back from temp table and delete temp table
        let addon_pop = Statement::from_string(
            DatabaseBackend::Sqlite,
            r#"INSERT INTO addon SELECT * FROM addon_tmp;"#.to_owned(),
        );
        db.execute(addon_pop).await?;
        let drop_tmp = Statement::from_string(
            DatabaseBackend::Sqlite,
            r#"DROP TABLE addon_tmp;"#.to_owned(),
        );
        db.execute(drop_tmp).await?;
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            r#"PRAGMA foreign_keys = ON;"#.to_owned(),
        ))
        .await?;

        // Populate base parent categories. For some reason they are not listed in the main categories list.
        // 0: "All"     // Not sure what it's for. Seems to be a parent of every category.
        // 23: "Stand-Alone Addons"
        // 39: "Class & Role Specific"
        // 144: "Utilities"
        // 154: "Optional"
        let base_cats = [
            (0, "All"),
            (23, "Stand-Alone Addons"),
            (39, "Class & Role Specific"),
            (144, "Utilities"),
            (154, "Optional"),
        ];
        let mut insert = Query::insert()
            .into_table(Category::Table)
            .columns([Category::Id, Category::Title])
            .to_owned();
        for cat in base_cats.iter() {
            insert.values_panic(vec![cat.0.into(), cat.1.into()]);
        }
        manager.exec_stmt(insert).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CategoryParent::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Category::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(AddonDetail::Table).to_owned())
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum Category {
    Table,
    Id,
    Title,
    Icon,
    FileCount,
}

#[derive(Iden)]
enum CategoryParent {
    Table,
    Id,
    ParentId,
}

#[derive(Iden)]
pub enum AddonDetail {
    Table,
    Id,
    Description,
    ChangeLog,
}
