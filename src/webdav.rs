use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use url::Url;

use crate::config::Config;
use crate::error::AppError;
use crate::watcher;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub server_ip: String,
    pub event_tx: broadcast::Sender<watcher::FsEvent>,
    pub _watchers: Arc<watcher::WatcherManager>,
}

type AppResult = Result<Response, AppError>;

fn resolve_path(
    url_path: &str,
    config: &Config,
) -> Result<(String, PathBuf, PathBuf), AppError> {
    let path = url_path.trim_start_matches('/');
    if path.is_empty() {
        return Err(AppError::NotFound("Invalid path".to_string()));
    }

    let parts: Vec<&str> = path.splitn(2, '/').collect();
    let share_name = parts[0];
    let relative = parts.get(1).copied().unwrap_or("");

    let share = config
        .share_by_name(share_name)
        .ok_or_else(|| AppError::NotFound(format!("Share '{}' not found", share_name)))?;

    let share_path = PathBuf::from(&share.path);
    let relative_path = if relative.is_empty() {
        PathBuf::new()
    } else {
        let decoded = urlencoding::decode(relative)
            .map_err(|e| AppError::Internal(format!("URL decode error: {}", e)))?;
        PathBuf::from(decoded.as_ref())
    };

    let full_path = share_path.join(&relative_path);

    let canonical_base = std::fs::canonicalize(&share_path)
        .map_err(|_| AppError::NotFound(format!("Share path not found: {}", share_path.display())))?;

    if relative_path.components().count() > 0 {
        if full_path.exists() {
            let canonical_full = std::fs::canonicalize(&full_path).map_err(|_| {
                AppError::NotFound(format!("Path not found: {}", full_path.display()))
            })?;
            if !canonical_full.starts_with(&canonical_base) {
                return Err(AppError::Forbidden("Path traversal detected".to_string()));
            }
        }
    }

    Ok((share_name.to_string(), relative_path, full_path))
}

fn format_http_date(time: SystemTime) -> String {
    let datetime: DateTime<Utc> = time.into();
    datetime.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

fn compute_etat(metadata: &std::fs::Metadata) -> String {
    let mut hasher = Sha256::new();
    hasher.update(metadata.len().to_be_bytes());
    if let Ok(mtime) = metadata.modified() {
        if let Ok(duration) = mtime.duration_since(std::time::UNIX_EPOCH) {
            hasher.update(duration.as_nanos().to_be_bytes());
        }
    }
    format!("\"{}\"", hex::encode(hasher.finalize()))
}

fn get_content_type(path: &std::path::Path) -> String {
    if path.is_dir() {
        "httpd/unix-directory".to_string()
    } else {
        mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string()
    }
}

pub async fn handler(
    State(state): State<AppState>,
    req: Request,
) -> AppResult {
    let path = req.uri().path().trim_start_matches('/').to_string();
    let method = req.method().clone();
    let (parts, body) = req.into_parts();
    let headers = parts.headers.clone();

    let body_bytes = match axum::body::to_bytes(body, 50 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return Err(AppError::Internal(format!("Failed to read body: {}", e)));
        }
    };

    let st = state.clone();
    let p = path.clone();

    match method {
        Method::OPTIONS => options_handler(st, p).await,
        Method::GET => get_handler(st, p).await,
        Method::HEAD => head_handler(st, p).await,
        Method::PUT => put_handler(st, p, body_bytes).await,
        Method::DELETE => delete_handler(st, p).await,
        m if m.as_str() == "PROPFIND" => propfind_handler(st, p, headers, &body_bytes).await,
        m if m.as_str() == "MKCOL" => mkcol_handler(st, p).await,
        m if m.as_str() == "COPY" => copy_handler(st, p, headers).await,
        m if m.as_str() == "MOVE" => move_handler(st, p, headers).await,
        m if m.as_str() == "PROPPATCH" => proppatch_handler().await,
        _ => {
            let resp = Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body(axum::body::Body::from("Method not allowed"))
                .unwrap();
            Ok(resp)
        }
    }
}

async fn options_handler(
    state: AppState,
    path: String,
) -> AppResult {
    let allowed = "OPTIONS, GET, HEAD, PUT, DELETE, PROPFIND, PROPPATCH, MKCOL, COPY, MOVE";

    if !path.is_empty() {
        let _ = resolve_path(&path, &state.config)?;
    }

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("DAV", "1, 2")
        .header("Allow", allowed)
        .header("Content-Length", "0")
        .body(axum::body::Body::empty())
        .unwrap();
    Ok(resp)
}

async fn get_handler(
    state: AppState,
    path: String,
) -> AppResult {
    let (_, _, full_path) = resolve_path(&path, &state.config)?;

    if !full_path.exists() {
        return Err(AppError::NotFound(format!(
            "File not found: {}",
            full_path.display()
        )));
    }

    if full_path.is_dir() {
        return list_directory(&path, &full_path).await;
    }

    let metadata = tokio::fs::metadata(&full_path).await?;
    let content_type = get_content_type(&full_path);
    let etag = compute_etat(&metadata);
    let modified = metadata
        .modified()
        .map(|t| format_http_date(t))
        .unwrap_or_default();
    let content = tokio::fs::read(&full_path).await?;

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", content_type)
        .header("Content-Length", content.len().to_string())
        .header("ETag", etag)
        .header("Last-Modified", modified)
        .header("Accept-Ranges", "bytes")
        .body(axum::body::Body::from(content))
        .unwrap();
    Ok(resp)
}

async fn list_directory(_url_path: &str, _dir_path: &std::path::Path) -> AppResult {
    let html = include_str!("browser.html");
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(axum::body::Body::from(html))
        .unwrap();
    Ok(resp)
}

async fn head_handler(
    state: AppState,
    path: String,
) -> AppResult {
    let (_, _, full_path) = resolve_path(&path, &state.config)?;

    if !full_path.exists() {
        return Err(AppError::NotFound(format!(
            "File not found: {}",
            full_path.display()
        )));
    }

    let metadata = tokio::fs::metadata(&full_path).await?;
    let content_type = get_content_type(&full_path);
    let etag = compute_etat(&metadata);
    let modified = metadata
        .modified()
        .map(|t| format_http_date(t))
        .unwrap_or_default();

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", content_type)
        .header("Content-Length", metadata.len().to_string())
        .header("ETag", etag)
        .header("Last-Modified", modified)
        .body(axum::body::Body::empty())
        .unwrap();
    Ok(resp)
}

async fn put_handler(
    state: AppState,
    path: String,
    body: Bytes,
) -> AppResult {
    let (_, _, full_path) = resolve_path(&path, &state.config)?;

    if let Some(parent) = full_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    tokio::fs::write(&full_path, &body).await?;
    let _ = state.event_tx.send(watcher::FsEvent { kind: "modify".to_string() });

    let metadata = tokio::fs::metadata(&full_path).await?;
    let etag = compute_etat(&metadata);

    let resp = Response::builder()
        .status(StatusCode::CREATED)
        .header("ETag", etag)
        .body(axum::body::Body::from("Created"))
        .unwrap();
    Ok(resp)
}

async fn delete_handler(
    state: AppState,
    path: String,
) -> AppResult {
    let (_, _, full_path) = resolve_path(&path, &state.config)?;

    if !full_path.exists() {
        return Err(AppError::NotFound(format!(
            "Path not found: {}",
            full_path.display()
        )));
    }

    if full_path.is_dir() {
        tokio::fs::remove_dir_all(&full_path).await?;
    } else {
        tokio::fs::remove_file(&full_path).await?;
    }
    let _ = state.event_tx.send(watcher::FsEvent { kind: "remove".to_string() });

    let resp = Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .unwrap();
    Ok(resp)
}

async fn mkcol_handler(
    state: AppState,
    path: String,
) -> AppResult {
    let (_, _, full_path) = resolve_path(&path, &state.config)?;

    if full_path.exists() {
        return Err(AppError::Internal("Collection already exists".to_string()));
    }

    tokio::fs::create_dir(&full_path).await?;
    let _ = state.event_tx.send(watcher::FsEvent { kind: "create".to_string() });

    let resp = Response::builder()
        .status(StatusCode::CREATED)
        .body(axum::body::Body::from("Created"))
        .unwrap();
    Ok(resp)
}

fn props_to_xml(
    href: &str,
    path: &std::path::Path,
    metadata: &std::fs::Metadata,
) -> Result<String, AppError> {
    let is_dir = path.is_dir();
    let content_type = get_content_type(path);
    let content_length = if is_dir { 0 } else { metadata.len() };
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let etag = compute_etat(metadata);
    let display_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut xml = String::new();
    xml.push_str("<D:response><D:href>");
    xml.push_str(&xml_escape(href));
    xml.push_str("</D:href><D:propstat><D:prop>");
    xml.push_str(&format!(
        "<D:displayname>{}</D:displayname>",
        xml_escape(&display_name)
    ));
    xml.push_str(&format!(
        "<D:getcontenttype>{}</D:getcontenttype>",
        xml_escape(&content_type)
    ));
    xml.push_str(&format!("<D:getcontentlength>{}</D:getcontentlength>", content_length));
    xml.push_str(&format!(
        "<D:getlastmodified>{}</D:getlastmodified>",
        xml_escape(&format_http_date(modified))
    ));
    xml.push_str(&format!("<D:getetag>{}</D:getetag>", xml_escape(&etag)));
    if is_dir {
        xml.push_str("<D:resourcetype><D:collection/></D:resourcetype>");
    } else {
        xml.push_str("<D:resourcetype/>");
    }
    xml.push_str("</D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>");
    Ok(xml)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

async fn propfind_handler(
    state: AppState,
    path: String,
    headers: HeaderMap,
    _body: &[u8],
) -> AppResult {
    let (share_name, relative_path, full_path) = resolve_path(&path, &state.config)?;

    if !full_path.exists() {
        return Err(AppError::NotFound(format!(
            "Path not found: {}",
            full_path.display()
        )));
    }

    let depth = headers
        .get("Depth")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("0");

    let base_url_path = format!("/{}", share_name);
    let rel_path_str = if relative_path.components().count() > 0 {
        let mut p = String::new();
        for comp in relative_path.components() {
            p.push('/');
            p.push_str(&urlencoding::encode(&comp.as_os_str().to_string_lossy()));
        }
        p
    } else {
        String::new()
    };

    let url_path = format!(
        "{}{}{}",
        base_url_path,
        rel_path_str,
        if full_path.is_dir() && !rel_path_str.is_empty() {
            "/"
        } else {
            ""
        }
    );

    let mut responses = Vec::new();
    let metadata = std::fs::metadata(&full_path)?;

    if depth == "0" {
        responses.push(props_to_xml(
            &if url_path.is_empty() { "/" } else { &url_path },
            &full_path,
            &metadata,
        )?);
    } else {
        responses.push(props_to_xml(
            &if url_path.is_empty() { "/" } else { &url_path },
            &full_path,
            &metadata,
        )?);

        if full_path.is_dir() {
            let mut entries = tokio::fs::read_dir(&full_path).await?;
            let mut children = Vec::new();
            while let Some(entry) = entries.next_entry().await? {
                children.push(entry);
            }
            children.sort_by_key(|e| e.file_name());

            for entry in &children {
                let name = entry.file_name();
                let child_path = full_path.join(&name);
                let name_str = name.to_string_lossy().to_string();
                let name_encoded = urlencoding::encode(&name_str);
                let child_url = format!(
                    "{}/{}{}",
                    url_path.trim_end_matches('/'),
                    name_encoded,
                    if child_path.is_dir() { "/" } else { "" }
                );
                let child_metadata = std::fs::metadata(&child_path)?;
                responses.push(props_to_xml(&child_url, &child_path, &child_metadata)?);
            }
        }
    }

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <D:multistatus xmlns:D=\"DAV:\">",
    );
    for resp in &responses {
        xml.push_str(resp);
    }
    xml.push_str("</D:multistatus>");

    let resp = Response::builder()
        .status(StatusCode::MULTI_STATUS)
        .header("Content-Type", "application/xml; charset=\"utf-8\"")
        .header("DAV", "1, 2")
        .body(axum::body::Body::from(xml))
        .unwrap();
    Ok(resp)
}

fn resolve_destination(
    dest_header: &str,
    config: &Config,
) -> Result<(String, PathBuf), AppError> {
    let dest_url = Url::parse(dest_header)
        .map_err(|_| AppError::Internal("Invalid Destination header".to_string()))?;
    let dest_path = dest_url.path().trim_start_matches('/').to_string();
    let (_, rel, full) = resolve_path(&dest_path, config)?;
    Ok((rel.to_string_lossy().to_string(), full))
}

async fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), AppError> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
}

async fn copy_move_inner(
    source: &std::path::Path,
    dest: &std::path::Path,
    overwrite: bool,
    is_move: bool,
) -> Result<StatusCode, AppError> {
    if dest.exists() {
        if overwrite {
            if dest.is_dir() {
                tokio::fs::remove_dir_all(dest).await?;
            } else {
                tokio::fs::remove_file(dest).await?;
            }
        } else {
            return Err(AppError::Internal("Destination already exists".to_string()));
        }
    }

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    if source.is_dir() {
        copy_dir_recursive(source, dest).await?;
    } else {
        tokio::fs::copy(source, dest).await?;
    }

    if is_move && source.exists() {
        if source.is_dir() {
            tokio::fs::remove_dir_all(source).await?;
        } else {
            tokio::fs::remove_file(source).await?;
        }
    }

    let status = if !dest.exists() {
        StatusCode::CREATED
    } else {
        StatusCode::NO_CONTENT
    };
    Ok(status)
}

async fn copy_handler(
    state: AppState,
    path: String,
    headers: HeaderMap,
) -> AppResult {
    let (_, _, source_path) = resolve_path(&path, &state.config)?;

    let dest_header = headers
        .get("Destination")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Internal("Missing Destination header".to_string()))?;

    let (_, dest_path) = resolve_destination(dest_header, &state.config)?;

    let overwrite = headers
        .get("Overwrite")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("T")
        .to_uppercase()
        .starts_with('T');

    if !source_path.exists() {
        return Err(AppError::NotFound("Source not found".to_string()));
    }

    let status = copy_move_inner(&source_path, &dest_path, overwrite, false).await?;
    let _ = state.event_tx.send(watcher::FsEvent { kind: "create".to_string() });

    let resp = Response::builder()
        .status(status)
        .body(axum::body::Body::empty())
        .unwrap();
    Ok(resp)
}

async fn move_handler(
    state: AppState,
    path: String,
    headers: HeaderMap,
) -> AppResult {
    let (_, _, source_path) = resolve_path(&path, &state.config)?;

    let dest_header = headers
        .get("Destination")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Internal("Missing Destination header".to_string()))?;

    let (_, dest_path) = resolve_destination(dest_header, &state.config)?;

    let overwrite = headers
        .get("Overwrite")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("T")
        .to_uppercase()
        .starts_with('T');

    if !source_path.exists() {
        return Err(AppError::NotFound("Source not found".to_string()));
    }

    let status = copy_move_inner(&source_path, &dest_path, overwrite, true).await?;
    let _ = state.event_tx.send(watcher::FsEvent { kind: "modify".to_string() });

    let resp = Response::builder()
        .status(status)
        .body(axum::body::Body::empty())
        .unwrap();
    Ok(resp)
}

async fn proppatch_handler() -> AppResult {
    Err(AppError::Internal("PROPPATCH not implemented".to_string()))
}
