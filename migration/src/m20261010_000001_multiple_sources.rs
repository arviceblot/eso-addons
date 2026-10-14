use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. git_repo - tracks Git repository registries
        manager
            .create_table(
                Table::create()
                    .table(GitRepo::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(GitRepo::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(GitRepo::Url)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(GitRepo::Branch)
                            .string()
                            .not_null()
                            .default("main"),
                    )
                    .col(ColumnDef::new(GitRepo::Path).string().not_null())
                    .col(ColumnDef::new(GitRepo::LastUpdated).string().null())
                    .col(
                        ColumnDef::new(GitRepo::CreatedAt)
                            .string()
                            .not_null()
                            .default("CURRENT_TIMESTAMP"),
                    )
                    .to_owned(),
            )
            .await?;

        // 2. git_addon - addons tracked by a git repo
        manager
            .create_table(
                Table::create()
                    .table(GitAddon::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(GitAddon::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(GitAddon::GitRepoId).integer().not_null())
                    .col(ColumnDef::new(GitAddon::Directory).string().not_null())
                    .col(ColumnDef::new(GitAddon::Title).string().not_null())
                    .col(ColumnDef::new(GitAddon::Author).string().null())
                    .col(ColumnDef::new(GitAddon::Version).string().null())
                    .col(ColumnDef::new(GitAddon::AddOnVersion).integer().null())
                    .col(ColumnDef::new(GitAddon::Description).string().null())
                    .col(ColumnDef::new(GitAddon::RemoteVersion).string().null())
                    .col(
                        ColumnDef::new(GitAddon::RemoteAddOnVersion)
                            .integer()
                            .null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_git_addon_repo")
                            .from(GitAddon::Table, GitAddon::GitRepoId)
                            .to(GitRepo::Table, GitRepo::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .to_owned(),
            )
            .await?;

        // Unique index on (git_repo_id, directory)
        manager
            .create_index(
                Index::create()
                    .unique()
                    .table(GitAddon::Table)
                    .col(GitAddon::GitRepoId)
                    .col(GitAddon::Directory)
                    .name("idx_git_addon_repo_dir")
                    .to_owned(),
            )
            .await?;

        // 3. local_addon - manually installed addons (no remote source)
        manager
            .create_table(
                Table::create()
                    .table(LocalAddon::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(LocalAddon::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(LocalAddon::Directory)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(LocalAddon::Title).string().not_null())
                    .col(ColumnDef::new(LocalAddon::Author).string().null())
                    .col(ColumnDef::new(LocalAddon::Version).string().null())
                    .col(ColumnDef::new(LocalAddon::AddOnVersion).integer().null())
                    .col(ColumnDef::new(LocalAddon::Description).string().null())
                    .to_owned(),
            )
            .await?;

        // 4. installed_local_addon - installation state for local addons
        manager
            .create_table(
                Table::create()
                    .table(InstalledLocalAddon::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(InstalledLocalAddon::LocalAddonId)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(InstalledLocalAddon::InstalledVersion)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(InstalledLocalAddon::InstalledAddOnVersion)
                            .integer()
                            .null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_installed_local_addon")
                            .from(
                                InstalledLocalAddon::Table,
                                InstalledLocalAddon::LocalAddonId,
                            )
                            .to(LocalAddon::Table, LocalAddon::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .to_owned(),
            )
            .await?;

        // 5. installed_git_addon - installation state for git addons
        manager
            .create_table(
                Table::create()
                    .table(InstalledGitAddon::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(InstalledGitAddon::GitAddonId)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(InstalledGitAddon::InstalledVersion)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(InstalledGitAddon::InstalledAddOnVersion)
                            .integer()
                            .null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("FK_installed_git_addon")
                            .from(InstalledGitAddon::Table, InstalledGitAddon::GitAddonId)
                            .to(GitAddon::Table, GitAddon::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop in reverse order (respecting foreign keys)
        manager
            .drop_table(Table::drop().table(InstalledGitAddon::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(InstalledLocalAddon::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(LocalAddon::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(GitAddon::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(GitRepo::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum GitRepo {
    Table,
    Id,
    Url,
    Branch,
    Path,
    LastUpdated,
    CreatedAt,
}

#[derive(DeriveIden)]
enum GitAddon {
    Table,
    Id,
    GitRepoId,
    Directory,
    Title,
    Author,
    Version,
    AddOnVersion,
    Description,
    RemoteVersion,
    RemoteAddOnVersion,
}

#[derive(DeriveIden)]
enum LocalAddon {
    Table,
    Id,
    Directory,
    Title,
    Author,
    Version,
    AddOnVersion,
    Description,
}

#[derive(DeriveIden)]
enum InstalledLocalAddon {
    Table,
    LocalAddonId,
    InstalledVersion,
    InstalledAddOnVersion,
}

#[derive(DeriveIden)]
enum InstalledGitAddon {
    Table,
    GitAddonId,
    InstalledVersion,
    InstalledAddOnVersion,
}
