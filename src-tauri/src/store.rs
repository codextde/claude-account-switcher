//! Persistent app state: accounts, per-account credential backups and settings.
//! Stored as one JSON file with owner-only permissions in the app data directory.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::credentials::atomic_write;
use crate::error::Result;
use crate::models::{Account, Backup, Settings};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct StateFile {
    pub version: u32,
    pub accounts: Vec<Account>,
    pub backups: HashMap<Uuid, Backup>,
    pub settings: Settings,
    pub active_id: Option<Uuid>,
}

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    pub data: StateFile,
}

impl Store {
    pub fn load(dir: PathBuf) -> Self {
        let path = dir.join("state.json");
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<StateFile>(&raw).ok())
            .map(|mut d| {
                d.settings = d.settings.sanitized();
                d
            })
            .unwrap_or_else(|| StateFile {
                version: 1,
                ..Default::default()
            });
        Self { path, data }
    }

    pub fn save(&self) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.data)?;
        atomic_write(&self.path, &bytes)
    }

    pub fn account(&self, id: Uuid) -> Option<&Account> {
        self.data.accounts.iter().find(|a| a.id == id)
    }

    pub fn account_mut(&mut self, id: Uuid) -> Option<&mut Account> {
        self.data.accounts.iter_mut().find(|a| a.id == id)
    }

    pub fn account_by_email(&self, email: &str) -> Option<&Account> {
        let email = email.trim().to_lowercase();
        self.data.accounts.iter().find(|a| a.email.to_lowercase() == email)
    }

    pub fn active(&self) -> Option<&Account> {
        self.data.active_id.and_then(|id| self.account(id))
    }

    pub fn backup(&self, id: Uuid) -> Option<&Backup> {
        self.data.backups.get(&id)
    }

    pub fn set_backup(&mut self, id: Uuid, backup: Backup) {
        self.data.backups.insert(id, backup);
    }

    pub fn remove_account(&mut self, id: Uuid) {
        self.data.accounts.retain(|a| a.id != id);
        self.data.backups.remove(&id);
        if self.data.active_id == Some(id) {
            self.data.active_id = None;
        }
    }
}
