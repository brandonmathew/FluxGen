//! SQLite store for gallery records, presets, and the LoRA
//! library.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use fluxgen_core::{AppError, LoraEntry, Preset, Quant, Variant};

fn storage_err(e: rusqlite::Error) -> AppError {
    AppError::Storage(format!("database error: {e}"))
}

fn json_err(e: serde_json::Error) -> AppError {
    AppError::Storage(format!("record serialization error: {e}"))
}

/// One gallery row: an output image and the full record of how it
/// was generated.
#[derive(Debug, Clone, PartialEq)]
pub struct GalleryRecord {
    /// Database row id; `None` before insertion.
    pub id: Option<i64>,
    /// Output image path.
    pub path: PathBuf,
    /// Unix timestamp (seconds) of creation.
    pub created_at: i64,
    /// Variant used.
    pub variant: Variant,
    /// Quantization used.
    pub quant: Quant,
    /// Generation settings as JSON (the same document embedded in
    /// the PNG).
    pub settings_json: String,
}

/// The FluxGen database. One instance wraps one SQLite connection
/// to the database file inside the portable data root.
pub struct Store {
    conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS gallery (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    variant TEXT NOT NULL,
    quant TEXT NOT NULL,
    settings_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS presets (
    name TEXT PRIMARY KEY,
    preset_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS loras (
    name TEXT NOT NULL,
    variant TEXT NOT NULL,
    path TEXT NOT NULL,
    weight REAL NOT NULL,
    enabled INTEGER NOT NULL,
    PRIMARY KEY (variant, name)
);
";

impl Store {
    /// Open (creating if needed) the database at `path` and apply
    /// the schema.
    pub fn open(path: &Path) -> Result<Store, AppError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).map_err(storage_err)?;
        conn.execute_batch(SCHEMA).map_err(storage_err)?;
        Ok(Store { conn })
    }

    /// Open an in-memory database. Used by tests.
    pub fn open_in_memory() -> Result<Store, AppError> {
        let conn = Connection::open_in_memory().map_err(storage_err)?;
        conn.execute_batch(SCHEMA).map_err(storage_err)?;
        Ok(Store { conn })
    }

    // -- Gallery ---------------------------------------------------

    /// Insert a gallery record; returns the stored record with its
    /// row id.
    pub fn add_gallery_record(&self, record: &GalleryRecord) -> Result<GalleryRecord, AppError> {
        self.conn
            .execute(
                "INSERT INTO gallery (path, created_at, variant, quant, settings_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    record.path.to_string_lossy(),
                    record.created_at,
                    record.variant.id(),
                    record.quant.label(),
                    record.settings_json,
                ],
            )
            .map_err(storage_err)?;
        let mut stored = record.clone();
        stored.id = Some(self.conn.last_insert_rowid());
        Ok(stored)
    }

    /// All gallery records, newest first.
    pub fn list_gallery(&self) -> Result<Vec<GalleryRecord>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, path, created_at, variant, quant, settings_json
                 FROM gallery ORDER BY created_at DESC, id DESC",
            )
            .map_err(storage_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(storage_err)?;
        let mut records = Vec::new();
        for row in rows {
            let (id, path, created_at, variant_id, quant_label, settings_json) =
                row.map_err(storage_err)?;
            let variant = Variant::from_id(&variant_id).ok_or_else(|| {
                AppError::Storage(format!("unknown variant in gallery row: {variant_id}"))
            })?;
            let quant = quant_from_label(&quant_label).ok_or_else(|| {
                AppError::Storage(format!("unknown quant in gallery row: {quant_label}"))
            })?;
            records.push(GalleryRecord {
                id: Some(id),
                path: PathBuf::from(path),
                created_at,
                variant,
                quant,
                settings_json,
            });
        }
        Ok(records)
    }

    // -- Presets ---------------------------------------------------

    /// Insert or replace a preset by name.
    pub fn save_preset(&self, preset: &Preset) -> Result<(), AppError> {
        preset.validate()?;
        let json = serde_json::to_string(preset).map_err(json_err)?;
        self.conn
            .execute(
                "INSERT INTO presets (name, preset_json) VALUES (?1, ?2)
                 ON CONFLICT(name) DO UPDATE SET preset_json = excluded.preset_json",
                params![preset.name, json],
            )
            .map_err(storage_err)?;
        Ok(())
    }

    /// All presets, ordered by name.
    pub fn list_presets(&self) -> Result<Vec<Preset>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT preset_json FROM presets ORDER BY name")
            .map_err(storage_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage_err)?;
        let mut presets = Vec::new();
        for row in rows {
            let json = row.map_err(storage_err)?;
            presets.push(serde_json::from_str(&json).map_err(json_err)?);
        }
        Ok(presets)
    }

    /// Delete a preset by name. Returns whether a row was removed.
    pub fn delete_preset(&self, name: &str) -> Result<bool, AppError> {
        let removed = self
            .conn
            .execute("DELETE FROM presets WHERE name = ?1", params![name])
            .map_err(storage_err)?;
        Ok(removed > 0)
    }

    // -- LoRA library ----------------------------------------------

    /// Insert or replace a LoRA library entry.
    pub fn save_lora(&self, entry: &LoraEntry) -> Result<(), AppError> {
        entry.validate()?;
        self.conn
            .execute(
                "INSERT INTO loras (name, variant, path, weight, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(variant, name) DO UPDATE SET
                     path = excluded.path,
                     weight = excluded.weight,
                     enabled = excluded.enabled",
                params![
                    entry.name,
                    entry.variant.id(),
                    entry.path.to_string_lossy(),
                    entry.weight,
                    entry.enabled as i64,
                ],
            )
            .map_err(storage_err)?;
        Ok(())
    }

    /// LoRA entries for one variant, ordered by name.
    pub fn list_loras(&self, variant: Variant) -> Result<Vec<LoraEntry>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name, path, weight, enabled FROM loras
                 WHERE variant = ?1 ORDER BY name",
            )
            .map_err(storage_err)?;
        let rows = stmt
            .query_map(params![variant.id()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f32>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(storage_err)?;
        let mut entries = Vec::new();
        for row in rows {
            let (name, path, weight, enabled) = row.map_err(storage_err)?;
            entries.push(LoraEntry {
                name,
                path: PathBuf::from(path),
                variant,
                weight,
                enabled: enabled != 0,
            });
        }
        Ok(entries)
    }

    /// Delete a LoRA entry. Returns whether a row was removed.
    pub fn delete_lora(&self, variant: Variant, name: &str) -> Result<bool, AppError> {
        let removed = self
            .conn
            .execute(
                "DELETE FROM loras WHERE variant = ?1 AND name = ?2",
                params![variant.id(), name],
            )
            .map_err(storage_err)?;
        Ok(removed > 0)
    }
}

/// Parse a stored quant label back into a [`Quant`].
fn quant_from_label(label: &str) -> Option<Quant> {
    [
        Quant::Q2,
        Quant::Q3,
        Quant::Q4,
        Quant::Q5,
        Quant::Q6,
        Quant::Q8,
        Quant::Bf16,
    ]
    .into_iter()
    .find(|q| q.label() == label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxgen_core::GenerationSettings;

    fn record() -> GalleryRecord {
        GalleryRecord {
            id: None,
            path: PathBuf::from("outputs/img-001.png"),
            created_at: 1_700_000_000,
            variant: Variant::ChromaHd,
            quant: Quant::Q6,
            settings_json: "{\"prompt\":\"test\"}".to_string(),
        }
    }

    #[test]
    fn gallery_round_trip() {
        let store = Store::open_in_memory().expect("store");
        let stored = store.add_gallery_record(&record()).expect("insert");
        assert!(stored.id.is_some());
        let listed = store.list_gallery().expect("list");
        assert_eq!(listed, vec![stored]);
    }

    #[test]
    fn preset_round_trip_and_delete() {
        let store = Store::open_in_memory().expect("store");
        let settings = GenerationSettings {
            prompt: "harbor".to_string(),
            ..GenerationSettings::default()
        };
        let preset = Preset {
            name: "Harbor".to_string(),
            variant: Variant::Flux1Dev,
            settings,
            loras: vec![],
        };
        store.save_preset(&preset).expect("save");
        assert_eq!(store.list_presets().expect("list"), vec![preset]);
        assert!(store.delete_preset("Harbor").expect("delete"));
        assert!(store.list_presets().expect("list").is_empty());
    }

    #[test]
    fn lora_library_is_per_variant() {
        let store = Store::open_in_memory().expect("store");
        let entry = LoraEntry::new(
            "detail",
            PathBuf::from("loras/detail.safetensors"),
            Variant::ChromaHd,
        );
        store.save_lora(&entry).expect("save");
        assert_eq!(
            store.list_loras(Variant::ChromaHd).expect("list"),
            vec![entry]
        );
        assert!(store
            .list_loras(Variant::Flux1Dev)
            .expect("list")
            .is_empty());
        assert!(store
            .delete_lora(Variant::ChromaHd, "detail")
            .expect("delete"));
    }

    #[test]
    fn file_backed_store_persists() {
        let dir = std::env::temp_dir().join(format!("fluxgen-store-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db_path = dir.join("fluxgen.db");
        {
            let store = Store::open(&db_path).expect("open");
            store.add_gallery_record(&record()).expect("insert");
        }
        let store = Store::open(&db_path).expect("reopen");
        assert_eq!(store.list_gallery().expect("list").len(), 1);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
