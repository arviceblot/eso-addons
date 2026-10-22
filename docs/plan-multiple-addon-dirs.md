# Plan: Multiple Addon Directories (Live / PTS)

## Problem

ESO Addon Manager currently supports only a single addon directory (`Config.addon_dir`).
Many players run both **Live** and **PTS** (Public Test Server) versions of ESO and need
to manage addons for both. Today, switching between them means changing the folder in
Settings, losing all context for the previously selected directory.

## Goal

Allow the user to register multiple addon directories (e.g. Live, PTS) and switch between
them with a dropdown. Each installed addon record references the directory it belongs to.

---

## Database

### New Table: `addon_folders`

```sql
CREATE TABLE addon_folders (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    label      TEXT    NOT NULL,       -- e.g. "Live", "PTS", custom
    path       TEXT    NOT NULL UNIQUE, -- absolute filesystem path
    is_default INTEGER NOT NULL DEFAULT 0, -- only one row may be 1
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

- `is_default` ensures exactly one folder is the "active default."
- `path` is unique so the same directory can't be registered twice.

### Foreign Key Columns

Add an `addon_folder_id INTEGER REFERENCES addon_folders(id)` column to:

| Existing Table        | Purpose                                          |
|-----------------------|--------------------------------------------------|
| `local_addons`        | Locally installed addon files belong to a folder |
| `git_addons`          | Cloned addons belong to a folder                 |
| `esoui_addons`        | ESOUI-downloaded addons belong to a folder       |

**Migration strategy:** When adding the column, backfill existing rows by inserting a
row into `addon_folders` from `Config.addon_dir` (label "Live", `is_default = 1`) and
setting all existing `addon_folder_id` values to its ID. This is a data migration within
the same migration file, not a separate step.

### Migration File

`migration/src/m<YYYYMMDD>_<HHMMSS>_multiple_addon_folders.rs`

Steps:

1. CREATE TABLE `addon_folders`
2. INSERT seed row from existing `config.addon_dir` value (read at runtime via the
   migration `up()` method — see existing migrations for the pattern).
3. ALTER TABLE `local_addons` ADD COLUMN `addon_folder_id`
4. ALTER TABLE `git_addons` ADD COLUMN `addon_folder_id`
5. ALTER TABLE `esoui_addons` ADD COLUMN `addon_folder_id`
6. UPDATE all existing rows to point to the seed folder ID.
7. In `down()`, reverse in order: DROP columns, DROP table.

After migration, regenerate entities:

```bash
sea-orm-cli generate entity -o entity/src
```

---

## Config Changes

### `Config` struct (`core/src/config.rs`)

Add an optional field:

```rust
/// ID of the currently active addon folder.
/// When None, falls back to the folder where `is_default = 1` in the DB.
pub active_addon_folder_id: Option<i64>,
```

- Keep `addon_dir: PathBuf` for backward compatibility (existing configs without
  `active_addon_folder_id` still work).
- On first boot after migration, if `active_addon_folder_id` is `None`, the service
  resolves the default from the DB.

---

## Service Layer Changes

### New Methods (`core/src/service/mod.rs`)

| Method | Signature | Purpose |
|--------|-----------|---------|
| `init_default_folder()` | `async fn(&mut self) -> Result<()>` | On startup, check if `addon_folders` is empty. If so, seed from `Config.addon_dir`. |
| `get_active_addon_dir()` | `async fn(&self) -> Result<PathBuf>` | Replace `get_addon_dir()`. Resolve `active_addon_folder_id` → DB row → `path`. |
| `get_addon_folders()` | `async fn(&self) -> Result<Vec<AddonFolder>>` | List all registered folders. |
| `add_addon_folder()` | `async fn(&self, label: String, path: PathBuf) -> Result<i64>` | Insert a new folder row. Set `is_default = 0`. |
| `set_default_addon_folder()` | `async fn(&self, id: i64) -> Result<()>` | Clear `is_default` on all, set on target. Update config. |
| `remove_addon_folder()` | `async fn(&self, id: i64) -> Result<()>` | Delete folder if it has no associated addons. If default, refuse. |

### Updating `get_addon_dir()` Callers

All ~13 call sites to `get_addon_dir()` should be migrated to `get_active_addon_dir()`.
This is a mechanical rename, but each caller needs to be checked for whether the result
is used in a filter query (it should be).

### Filter Queries

Installed addon list queries must include:

```sql
... WHERE addon_folder_id = ?
```

This applies to `local_addons`, `git_addons`, and `esoui_addons` queries used by the
installed view.

---

## UI Changes

### Left Sidebar — Folder Dropdown

**Location:** Top of the left sidebar panel in the main window (`src/main.rs`).

**Behavior:**

- A `egui::ComboBox` dropdown showing all registered addon folder labels.
- The currently active folder is selected (default marker or bold).
- Clicking a label calls `set_default_addon_folder(id)` and triggers a refresh.
- Dropdown always shows the current active folder as the preview text.

### Left Sidebar — "+ Add Folder" Button

**Location:** Bottom of the left sidebar panel.

**Behavior:**

1. Button opens an `rfd::AsyncFileDialog::pick_folder()` dialog (same pattern as
   onboard/settings, using `ImmediateValuePromise`).
2. On folder selection, run the Live→PTS path substitution heuristic (see below).
3. If a PTS path is detected, pre-fill a text input for the label with `"PTS"`.
4. Show a small inline form (label text field + Confirm/Cancel) before inserting.
5. On confirm, call `add_addon_folder(label, path)` and update the dropdown.

### Live → PTS Path Substitution

**Heuristic:**

```
If the selected path contains "live" (case-insensitive):
    auto-suggest PTS path by replacing "live" with "pts" in the path
    default label = "PTS"
Else if the selected path contains "pts" (case-insensitive):
    default label = "PTS"
Else:
    default label = folder basename or "Custom"
```

The user can edit the label freely. The suggested PTS path is a hint shown in the
inline form; user must still confirm the actual selected path.

### Installed View — Filter by Folder

`src/views/installed.rs` already receives the addon list from the service. The service
layer will now filter by `active_addon_folder_id` before returning, so the view needs
no structural change. It may optionally display the active folder label as a breadcrumb
or subtitle for clarity.

---

## Implementation Order

1. **Migration** — Create `addon_folders` table, add foreign key columns, backfill data.
2. **Entity** — Run `sea-orm-cli generate entity`.
3. **Service** — Implement new methods, migrate `get_addon_dir()` → `get_active_addon_dir()`.
4. **Config** — Add `active_addon_folder_id` field.
5. **UI** — Add dropdown, "+ Add Folder" button, and inline label form.
6. **Tests** — Add unit tests for service methods and path substitution heuristic.

---

## Backward Compatibility

- Existing configs with only `addon_dir` work unchanged on first startup: the seed
  migration creates a folder from `addon_dir` labeled "Live."
- All existing addon records are automatically linked to the seed folder.
- No data loss. The feature is additive.
