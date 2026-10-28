use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::fs;
use std::path::PathBuf;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Canonical names for the addon_folders table and its columns.
#[derive(DeriveIden)]
enum AddonFolders {
    Table,
    Id,
    Label,
    Path,
    IsDefault,
    CreatedAt,
}

/// Column name used across local_addon, git_addon, and installed_addon.
const ADDON_FOLDER_ID: &str = "addon_folder_id";

/// Config file constants (mirrored from core/src/config.rs).
const EAM_DATA_DIR: &str = "eso-addons";
const EAM_CONF: &str = "config.json";

/// Read the `addon_dir` value from the JSON config file, falling back to `None`.
fn read_config_addon_dir() -> Option<PathBuf> {
    let config_dir = dirs::config_dir()?.join(EAM_DATA_DIR);
    let config_path = config_dir.join(EAM_CONF);
    let Ok(content) = fs::read_to_string(&config_path) else {
        return None;
    };
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json.get("addon_dir")?.as_str().map(PathBuf::from)
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. Create addon_folders table
        manager
            .create_table(
                Table::create()
                    .table(AddonFolders::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AddonFolders::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AddonFolders::Label).string().not_null())
                    .col(
                        ColumnDef::new(AddonFolders::Path)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(AddonFolders::IsDefault)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(AddonFolders::CreatedAt)
                            .string()
                            .not_null()
                            .default("CURRENT_TIMESTAMP"),
                    )
                    .to_owned(),
            )
            .await?;

        // 2. Seed a default folder from the config's addon_dir (label "Live").
        // If config cannot be read, use an empty path; the service will fix it on startup.
        let db = manager.get_connection();
        let seed_path = read_config_addon_dir().unwrap_or_default();
        let seed_path_str = seed_path.to_string_lossy().to_string();
        db.execute_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                r#"INSERT INTO "addon_folders" (label, "path", is_default) VALUES ('Live', '{escaped}', 1)"#,
                escaped = seed_path_str.replace('\'', "''"),
            ),
        ))
        .await?;

        // 3. Add addon_folder_id column to local_addon, git_addon, installed_addon.
        // SQLite doesn't support adding a REFERENCES column with non-NULL default,
        // so we add it nullable, backfill, and let the service enforce FK semantics.
        for table in &["local_addon", "git_addon", "installed_addon"] {
            db.execute_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!("ALTER TABLE {table} ADD COLUMN {ADDON_FOLDER_ID} INTEGER DEFAULT 1"),
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Drop addon_folder_id columns from tables.
        // SQLite < 3.35 doesn't support DROP COLUMN; fall back to table recreation.
        for table in &["installed_addon", "local_addon", "git_addon"] {
            let stmt = Statement::from_string(
                DatabaseBackend::Sqlite,
                format!("ALTER TABLE {table} DROP COLUMN {ADDON_FOLDER_ID}"),
            );
            match db.execute_raw(stmt).await {
                Ok(_) => continue,
                Err(e) => {
                    // SQLite < 3.35 doesn't support DROP COLUMN.
                    // Fall back to table recreation.
                    eprintln!(
                        "WARN: DROP COLUMN on `{table}` failed ({e}). Falling back to table recreation. \
                         Upgrade to SQLite >= 3.35 for native support."
                    );
                    drop_column_fallback(db, table, ADDON_FOLDER_ID).await?;
                }
            }
        }

        // Drop the addon_folders table
        manager
            .drop_table(Table::drop().table(AddonFolders::Table).to_owned())
            .await?;

        Ok(())
    }
}

/// Fallback for SQLite < 3.35 which doesn't support `ALTER TABLE ... DROP COLUMN`.
/// Recreates the table without the specified column by:
/// 1. Getting existing column info via PRAGMA
/// 2. Creating a new temp table without the target column
/// 3. Copying data, then swapping tables
async fn drop_column_fallback(
    db: &impl ConnectionTrait,
    table: &str,
    drop_col: &str,
) -> Result<(), DbErr> {
    // Collect columns excluding the one to drop
    let columns: Vec<String> = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info({table})"),
        ))
        .await?
        .iter()
        .filter_map(|row| {
            row.try_get("", "name")
                .ok()
                .filter(|n: &String| n != drop_col)
        })
        .collect();

    if columns.is_empty() {
        // Table has no other columns; just drop and recreate
        db.execute_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("DROP TABLE {table}"),
        ))
        .await?;
        return Ok(());
    }

    let col_list = columns.join(", ");
    let temp_table = format!("{table}_tmp");

    // Create new table without the target column
    db.execute_raw(Statement::from_string(
        DatabaseBackend::Sqlite,
        format!("CREATE TABLE {temp_table} AS SELECT {col_list} FROM {table}"),
    ))
    .await?;

    // Drop original and rename temp
    db.execute_raw(Statement::from_string(
        DatabaseBackend::Sqlite,
        format!("DROP TABLE {table}"),
    ))
    .await?;

    db.execute_raw(Statement::from_string(
        DatabaseBackend::Sqlite,
        format!("ALTER TABLE {temp_table} RENAME TO {table}"),
    ))
    .await?;

    Ok(())
}
