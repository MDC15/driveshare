use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::Path;
use tokio::sync::broadcast;
use tracing::info;

use crate::config::ShareConfig;

#[derive(Debug, Clone)]
pub struct FsEvent {
    pub kind: String,
}

pub struct WatcherManager {
    #[allow(dead_code)]
    watchers: Vec<notify::RecommendedWatcher>,
}

impl WatcherManager {
    pub fn new(shares: &[ShareConfig], tx: broadcast::Sender<FsEvent>) -> Self {
        let mut watchers = Vec::new();

        for share in shares {
            let path = Path::new(&share.path);
            if !path.exists() {
                continue;
            }

            let tx = tx.clone();
            let mut watcher = match notify::recommended_watcher(
                move |res: Result<Event, notify::Error>| {
                    if let Ok(event) = res {
                        let kind = match event.kind {
                            EventKind::Create(_) => "create",
                            EventKind::Modify(_) => "modify",
                            EventKind::Remove(_) => "remove",
                            _ => return,
                        };
                        info!(?kind, paths = ?event.paths, "File change detected");
                        let _ = tx.send(FsEvent { kind: kind.to_string() });
                    }
                },
            ) {
                Ok(w) => w,
                Err(e) => {
                    info!("Failed to create watcher for {}: {}", share.path, e);
                    continue;
                }
            };

            if let Err(e) = watcher.watch(path, RecursiveMode::Recursive) {
                info!("Failed to start watching {}: {}", share.path, e);
                continue;
            }

            info!("Watching directory for changes: {}", share.path);
            watchers.push(watcher);
        }

        WatcherManager { watchers }
    }
}
