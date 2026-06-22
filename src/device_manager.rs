use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

const ONLINE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub hostname: String,
    pub ip: String,
    pub last_seen: String,
    pub online: bool,
    pub view: bool,
    pub edit: bool,
    pub delete: bool,
    pub blocked: bool,
    #[serde(default)]
    pub is_self: bool,
}

struct DeviceEntry {
    info: DeviceInfo,
    last_seen_instant: Instant,
}

pub struct DeviceManager {
    devices: RwLock<HashMap<String, DeviceEntry>>,
}

impl DeviceManager {
    pub fn new() -> Self {
        DeviceManager {
            devices: RwLock::new(HashMap::new()),
        }
    }

    pub async fn record_device(&self, ip: &str, name: &str, hostname: &str) -> bool {
        let mut devices = self.devices.write().await;
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let is_new = !devices.contains_key(ip);
        devices
            .entry(ip.to_string())
            .and_modify(|e| {
                e.info.last_seen = now.clone();
                e.info.online = true;
                e.last_seen_instant = Instant::now();
                if !name.is_empty() && e.info.name == "Unknown" {
                    e.info.name = name.to_string();
                }
                if !hostname.is_empty() && e.info.hostname.is_empty() {
                    e.info.hostname = hostname.to_string();
                }
            })
            .or_insert(DeviceEntry {
                info: DeviceInfo {
                    name: if name.is_empty() {
                        "Unknown".to_string()
                    } else {
                        name.to_string()
                    },
                    hostname: if hostname.is_empty() {
                        String::new()
                    } else {
                        hostname.to_string()
                    },
                    ip: ip.to_string(),
                    last_seen: now,
                    online: true,
                    view: true,
                    edit: true,
                    delete: true,
                    blocked: false,
                    is_self: false,
                },
                last_seen_instant: Instant::now(),
            });
            is_new
    }

    fn update_online_status(devices: &mut HashMap<String, DeviceEntry>) {
        let cutoff = Instant::now() - ONLINE_TIMEOUT;
        for entry in devices.values_mut() {
            if entry.info.blocked {
                entry.info.online = false;
            } else {
                entry.info.online = entry.last_seen_instant >= cutoff;
            }
        }
    }

    pub async fn is_blocked(&self, ip: &str) -> bool {
        let devices = self.devices.read().await;
        devices.get(ip).map(|d| d.info.blocked).unwrap_or(false)
    }

    pub async fn check_permission(&self, ip: &str, action: &str) -> bool {
        let devices = self.devices.read().await;
        if let Some(entry) = devices.get(ip) {
            if entry.info.blocked {
                return false;
            }
            match action {
                "view" => entry.info.view,
                "edit" => entry.info.edit,
                "delete" => entry.info.delete,
                _ => true,
            }
        } else {
            true
        }
    }

    pub async fn block(&self, ip: &str) -> bool {
        let mut devices = self.devices.write().await;
        if let Some(entry) = devices.get_mut(ip) {
            entry.info.blocked = true;
            entry.info.online = false;
            true
        } else {
            false
        }
    }

    pub async fn unblock(&self, ip: &str) -> bool {
        let mut devices = self.devices.write().await;
        if let Some(entry) = devices.get_mut(ip) {
            entry.info.blocked = false;
            entry.info.online = entry.last_seen_instant + ONLINE_TIMEOUT >= Instant::now();
            true
        } else {
            false
        }
    }

    pub async fn update_permissions(&self, ip: &str, view: bool, edit: bool, delete: bool) -> bool {
        let mut devices = self.devices.write().await;
        if let Some(entry) = devices.get_mut(ip) {
            entry.info.view = view;
            entry.info.edit = edit;
            entry.info.delete = delete;
            true
        } else {
            false
        }
    }

    pub async fn list_devices(&self) -> Vec<DeviceInfo> {
        let mut devices = self.devices.write().await;
        Self::update_online_status(&mut devices);
        let mut list: Vec<DeviceInfo> = devices.values().map(|e| e.info.clone()).collect();
        list.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        list
    }

    pub async fn list_online_devices(&self) -> Vec<DeviceInfo> {
        let mut devices = self.devices.write().await;
        Self::update_online_status(&mut devices);
        let mut list: Vec<DeviceInfo> = devices
            .values()
            .filter(|e| e.info.online)
            .map(|e| e.info.clone())
            .collect();
        list.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        list
    }

    pub async fn resolve_hostname(ip: &str) -> String {
        let ip_str = ip.to_string();
        tokio::task::spawn_blocking(move || {
            let ip: IpAddr = match ip_str.parse() {
                Ok(ip) => ip,
                Err(_) => return String::new(),
            };
            match dns_lookup::lookup_addr(&ip) {
                Ok(name) => {
                    let name = name.trim_end_matches('.');
                    if name == ip_str {
                        String::new()
                    } else {
                        name.to_string()
                    }
                }
                Err(_) => String::new(),
            }
        })
        .await
        .unwrap_or_default()
    }
}
