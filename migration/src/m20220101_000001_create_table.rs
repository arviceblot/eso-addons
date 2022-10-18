use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Addon::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Addon::Id).integer().not_null().primary_key())
                    .col(ColumnDef::new(Addon::CategoryId).string().not_null())
                    .col(ColumnDef::new(Addon::Version).string().not_null())
                    .col(ColumnDef::new(Addon::Date).big_unsigned().not_null())
                    .col(ColumnDef::new(Addon::Name).string().not_null())
                    .col(ColumnDef::new(Addon::AuthorName).string())
                    .col(ColumnDef::new(Addon::FileInfoUrl).string())
                    .col(ColumnDef::new(Addon::DownloadTotal).string())
                    .col(ColumnDef::new(Addon::DownloadMonthly).string())
                    .col(ColumnDef::new(Addon::FavoriteTotal).string())
                    .col(ColumnDef::new(Addon::Md5).string())
                    .col(ColumnDef::new(Addon::FileName).string())
                    .col(ColumnDef::new(Addon::Download).string())
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(InstalledAddon::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(InstalledAddon::AddonId)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(InstalledAddon::Version).string().not_null())
                    .col(
                        ColumnDef::new(InstalledAddon::Date)
                            .big_unsigned()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_installed_addon")
                            .from(InstalledAddon::Table, InstalledAddon::AddonId)
                            .to(Addon::Table, Addon::Id)
                            .on_delete(ForeignKeyAction::NoAction)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(AddonDir::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(AddonDir::AddonId).integer().not_null())
                    .col(ColumnDef::new(AddonDir::Dir).string().not_null())
                    .primary_key(Index::create().col(AddonDir::AddonId).col(AddonDir::Dir))
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_addon_dir")
                            .from(AddonDir::Table, AddonDir::AddonId)
                            .to(Addon::Table, Addon::Id)
                            .on_delete(ForeignKeyAction::NoAction)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(AddonDependency::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AddonDependency::AddonId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AddonDependency::DependencyDir)
                            .string()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(AddonDependency::AddonId)
                            .col(AddonDependency::DependencyDir),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_addon_dependency_addon")
                            .from(AddonDependency::Table, AddonDependency::AddonId)
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
            .drop_table(Table::drop().table(InstalledAddon::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(AddonDir::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Addon::Table).to_owned())
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
enum Addon {
    Table,
    Id,
    CategoryId,
    Version,
    Date,
    Name,
    AuthorName,
    FileInfoUrl,
    DownloadTotal,
    DownloadMonthly,
    FavoriteTotal,
    Md5,
    FileName,
    Download,
}

#[derive(Iden)]
enum InstalledAddon {
    Table,
    AddonId,
    Version,
    Date,
}

#[derive(Iden)]
enum AddonDir {
    Table,
    AddonId,
    Dir,
}

#[derive(Iden)]
enum AddonDependency {
    Table,
    AddonId,
    DependencyDir,
}
