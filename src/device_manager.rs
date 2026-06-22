use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub ip: String,
    pub last_seen: String,
    pub view: bool,
    pub edit: bool,
    pub delete: bool,
    pub blocked: bool,
}

pub struct DeviceManager {
    devices: RwLock<HashMap<String, DeviceInfo>>,
}

impl DeviceManager {
    pub fn new() -> Self {
        DeviceManager {
            devices: RwLock::new(HashMap::new()),
        }
    }

    pub async fn record_device(&self, ip: &str, name: &str) {
        let mut devices = self.devices.write().await;
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        devices.entry(ip.to_string())
            .and_modify(|d| {
                d.last_seen = now.clone();
                if !name.is_empty() {
                    d.name = name.to_string();
                }
            })
            .or_insert(DeviceInfo {
                name: if name.is_empty() { "Unknown".to_string() } else { name.to_string() },
                ip: ip.to_string(),
                last_seen: now,
                view: true,
                edit: true,
                delete: true,
                blocked: false,
            });
    }

    pub async fn is_blocked(&self, ip: &str) -> bool {
        let devices = self.devices.read().await;
        devices.get(ip).map(|d| d.blocked).unwrap_or(false)
    }

    pub async fn check_permission(&self, ip: &str, action: &str) -> bool {
        let devices = self.devices.read().await;
        if let Some(device) = devices.get(ip) {
            if device.blocked {
                return false;
            }
            match action {
                "view" => device.view,
                "edit" => device.edit,
                "delete" => device.delete,
                _ => true,
            }
        } else {
            true
        }
    }

    pub async fn block(&self, ip: &str) -> bool {
        let mut devices = self.devices.write().await;
        if let Some(device) = devices.get_mut(ip) {
            device.blocked = true;
            true
        } else {
            false
        }
    }

    pub async fn unblock(&self, ip: &str) -> bool {
        let mut devices = self.devices.write().await;
        if let Some(device) = devices.get_mut(ip) {
            device.blocked = false;
            true
        } else {
            false
        }
    }

    pub async fn update_permissions(&self, ip: &str, view: bool, edit: bool, delete: bool) -> bool {
        let mut devices = self.devices.write().await;
        if let Some(device) = devices.get_mut(ip) {
            device.view = view;
            device.edit = edit;
            device.delete = delete;
            true
        } else {
            false
        }
    }

    pub async fn list_devices(&self) -> Vec<DeviceInfo> {
        let devices = self.devices.read().await;
        let mut list: Vec<DeviceInfo> = devices.values().cloned().collect();
        list.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        list
    }
}
