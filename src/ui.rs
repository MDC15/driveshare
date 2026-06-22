use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, Json, Response};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::io::Cursor;
use zip::write::FileOptions;
use zip::ZipWriter;

use crate::device_manager::DeviceInfo;
use crate::error::AppError;
use crate::webdav::AppState;

#[derive(Serialize)]
pub struct ServerIpInfo {
    pub ip: String,
    pub port: u16,
}

pub async fn api_ip(
    State(state): State<AppState>,
) -> Json<ServerIpInfo> {
    Json(ServerIpInfo {
        ip: state.server_ip.clone(),
        port: state.config.server.port,
    })
}

#[derive(Serialize)]
pub struct ShareInfo {
    pub name: String,
    pub path: String,
    pub description: String,
}

#[derive(Serialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
}

pub async fn dashboard() -> Html<&'static str> {
    Html(include_str!("dashboard.html"))
}

pub async fn api_shares(
    State(state): State<AppState>,
) -> Json<Vec<ShareInfo>> {
    let shares = state
        .config
        .shares
        .iter()
        .map(|s| ShareInfo {
            name: s.name.clone(),
            path: s.path.clone(),
            description: s.description.clone().unwrap_or_default(),
        })
        .collect();
    Json(shares)
}

pub async fn api_files(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<Json<Vec<FileEntry>>, AppError> {
    let path = path.trim_start_matches('/');
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    let share_name = parts[0];
    let rel_path = parts.get(1).copied().unwrap_or("");

    let share = state
        .config
        .share_by_name(share_name)
        .ok_or_else(|| AppError::NotFound(format!("Share '{}' not found", share_name)))?;

    let base_path = std::path::PathBuf::from(&share.path);
    let dir_path = if rel_path.is_empty() {
        base_path.clone()
    } else {
        base_path.join(rel_path)
    };

    if !dir_path.exists() || !dir_path.is_dir() {
        return Err(AppError::NotFound(format!(
            "Directory not found: {}",
            dir_path.display()
        )));
    }

    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&dir_path).await?;

    while let Some(entry) = read_dir.next_entry().await? {
        let metadata = entry.metadata().await?;
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        entries.push(FileEntry {
            modified: metadata
                .modified()
                .map(|t| {
                    let dt: chrono::DateTime<chrono::Utc> = t.into();
                    dt.format("%Y-%m-%d %H:%M:%S").to_string()
                })
                .unwrap_or_default(),
            name,
            is_dir: metadata.is_dir(),
            size: metadata.len(),
        });
    }

    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));

    Ok(Json(entries))
}

fn add_dir_to_zip(
    zip: &mut ZipWriter<Cursor<Vec<u8>>>,
    dir: &std::path::Path,
    prefix: &str,
) -> Result<(), AppError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let zip_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };

        if path.is_dir() {
            zip.add_directory(&format!("{}/", zip_path), FileOptions::default())
                .map_err(|e| AppError::Internal(e.to_string()))?;
            add_dir_to_zip(zip, &path, &zip_path)?;
        } else {
            zip.start_file(&zip_path, FileOptions::default())
                .map_err(|e| AppError::Internal(e.to_string()))?;
            let mut file =
                std::fs::File::open(&path).map_err(|e| AppError::Internal(e.to_string()))?;
            std::io::copy(&mut file, zip).map_err(|e| AppError::Internal(e.to_string()))?;
        }
    }
    Ok(())
}

pub async fn api_zip(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<Response, AppError> {
    let path = path.trim_start_matches('/');
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    let share_name = parts[0];
    let rel_path = parts.get(1).copied().unwrap_or("");

    let share = state
        .config
        .share_by_name(share_name)
        .ok_or_else(|| AppError::NotFound(format!("Share '{}' not found", share_name)))?;

    let base_path = std::path::PathBuf::from(&share.path);
    let dir_path = if rel_path.is_empty() {
        base_path.clone()
    } else {
        base_path.join(rel_path)
    };

    if !dir_path.exists() {
        return Err(AppError::NotFound(format!(
            "Path not found: {}",
            dir_path.display()
        )));
    }

    let dir_name = dir_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());

    let buf = Vec::new();
    let cursor = Cursor::new(buf);
    let mut zip = ZipWriter::new(cursor);

    if dir_path.is_dir() {
        zip.add_directory(&format!("{}/", dir_name), FileOptions::default())
            .map_err(|e| AppError::Internal(e.to_string()))?;
        add_dir_to_zip(&mut zip, &dir_path, &dir_name)?;
    } else {
        zip.start_file(&dir_name, FileOptions::default())
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let mut file =
            std::fs::File::open(&dir_path).map_err(|e| AppError::Internal(e.to_string()))?;
        std::io::copy(&mut file, &mut zip).map_err(|e| AppError::Internal(e.to_string()))?;
    }

    let cursor = zip.finish().map_err(|e| AppError::Internal(e.to_string()))?;
    let bytes = cursor.into_inner();

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/zip")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}.zip\"", dir_name),
        )
        .body(Body::from(bytes))
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(resp)
}

pub async fn api_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_tx.subscribe();
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if event.kind == "shutdown" {
                        return None;
                    }
                    let sse = Event::default().data("refresh");
                    return Some((Ok(sse), rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::new())
}

pub async fn api_devices_list(
    State(state): State<AppState>,
) -> Json<Vec<DeviceInfo>> {
    Json(state.device_manager.list_devices().await)
}

#[derive(Deserialize)]
pub struct BlockParams {
    ip: String,
}

pub async fn api_device_block(
    State(state): State<AppState>,
    Path(params): Path<BlockParams>,
) -> Result<Json<&'static str>, AppError> {
    if state.device_manager.block(&params.ip).await {
        Ok(Json("Blocked"))
    } else {
        Err(AppError::NotFound("Device not found".to_string()))
    }
}

pub async fn api_device_unblock(
    State(state): State<AppState>,
    Path(params): Path<BlockParams>,
) -> Result<Json<&'static str>, AppError> {
    if state.device_manager.unblock(&params.ip).await {
        Ok(Json("Unblocked"))
    } else {
        Err(AppError::NotFound("Device not found".to_string()))
    }
}

#[derive(Deserialize)]
pub struct PermissionBody {
    view: bool,
    edit: bool,
    delete: bool,
}

pub async fn api_device_permissions(
    State(state): State<AppState>,
    Path(params): Path<BlockParams>,
    Json(body): Json<PermissionBody>,
) -> Result<Json<&'static str>, AppError> {
    if state
        .device_manager
        .update_permissions(&params.ip, body.view, body.edit, body.delete)
        .await
    {
        Ok(Json("Updated"))
    } else {
        Err(AppError::NotFound("Device not found".to_string()))
    }
}
