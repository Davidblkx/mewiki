//! Uploaded assets: storing them, serving them so they can't act as the owner, and deleting them (R10).

use std::fs;
use std::io;
use std::path::Path as FsPath;
use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::api::ApiError;
use crate::App;
use crate::store::{self, PagePath};

/// The largest upload accepted, in bytes (R10).
pub const MAX_UPLOAD_BYTES: usize = 2 * 1024 * 1024;

/// File types shown in the browser with their real type. Everything else is sent as a download.
const INLINE_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("pdf", "application/pdf"),
    ("txt", "text/plain; charset=utf-8"),
];

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "avif"];

/// The name the browser gave the file.
#[derive(Deserialize)]
pub struct UploadQuery {
    name: String,
}

/// The answer to an upload: where the file lives and the Markdown that shows it.
#[derive(Serialize)]
pub struct Uploaded {
    name: String,
    url: String,
    markdown: String,
}

/// `POST /_/api/uploads?name=`: stores the request body under a cleaned-up version of `name`, adding `-1`, `-2`
/// and so on when the name is taken.
pub async fn upload(
    State(app): State<Arc<App>>,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Result<Json<Uploaded>, ApiError> {
    let _write = app.writes.lock().await;
    let task_app = app.clone();
    tokio::task::spawn_blocking(move || store_upload(&task_app, &query.name, &body))
        .await
        .map_err(|e| io::Error::other(e.to_string()))?
}

fn store_upload(app: &App, requested: &str, body: &[u8]) -> Result<Json<Uploaded>, ApiError> {
    let name = free_name(&app.data.uploads(), &clean_name(requested));
    store::write_atomic(&app.data.uploads().join(&name), body)?;
    let url = format!("/_/uploads/{name}");
    let markdown = if IMAGE_EXTENSIONS.contains(&extension(&name)) {
        format!("![{name}]({url})")
    } else {
        format!("[{name}]({url})")
    };
    Ok(Json(Uploaded { name, url, markdown }))
}

/// Turns a browser-supplied file name into one that is safe on disk and in a URL: a lowercase `[a-z0-9_-]` stem of
/// at most 90 characters and an optional extension of at most 10, with no folders and no leading dot.
pub fn clean_name(requested: &str) -> String {
    let base = requested.rsplit(['/', '\\']).next().unwrap_or_default().to_lowercase();
    let (stem, extension) = match base.rsplit_once('.') {
        Some((stem, extension)) => (stem, clean_part(extension, 10)),
        None => (base.as_str(), String::new()),
    };
    let mut stem = clean_part(stem, 90);
    if stem.is_empty() {
        stem = "file".to_owned();
    }
    if extension.is_empty() {
        stem
    } else {
        format!("{stem}.{extension}")
    }
}

fn clean_part(text: &str, max: usize) -> String {
    let mut cleaned = String::new();
    for c in text.chars() {
        let c = if c.is_ascii_alphanumeric() || c == '_' { c } else { '-' };
        if !(c == '-' && cleaned.ends_with('-')) {
            cleaned.push(c);
        }
    }
    let cleaned: String = cleaned.trim_matches('-').chars().take(max).collect();
    cleaned.trim_end_matches('-').to_owned()
}

fn free_name(folder: &FsPath, name: &str) -> String {
    if !folder.join(name).exists() {
        return name.to_owned();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    (1..)
        .map(|n| format!("{stem}-{n}{ext}"))
        .find(|candidate| !folder.join(candidate).exists())
        .expect("some suffix is free")
}

fn extension(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(_, ext)| ext)
}

fn is_upload_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

/// `GET /_/uploads/{name}`: an uploaded file, for anyone. Uploads are never protected (R13).
///
/// Every response is sandboxed and never sniffed. PNG, JPEG, GIF, WebP, PDF and plain text are shown with their real
/// type; everything else, SVG and HTML included, downloads as `application/octet-stream`. Either way an uploaded
/// file can't run scripts as the logged-in owner.
pub async fn serve(State(app): State<Arc<App>>, Path(name): Path<String>) -> Response {
    if !is_upload_name(&name) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let bytes = match tokio::fs::read(app.data.uploads().join(&name)).await {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return super::server_error(e),
    };
    let inline = INLINE_TYPES.iter().find(|(ext, _)| *ext == extension(&name));
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    match inline {
        Some((_, content_type)) => {
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        }
        None => {
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
                headers.insert(header::CONTENT_DISPOSITION, value);
            }
        }
    }
    response
}

/// `DELETE /_/api/uploads/{name}`: deletes an upload without checking whether pages use it; the dashboard marks
/// unused uploads.
pub async fn delete(State(app): State<Arc<App>>, Path(name): Path<String>) -> Result<StatusCode, ApiError> {
    if !is_upload_name(&name) {
        return Err(ApiError::not_found("No such upload."));
    }
    let _write = app.writes.lock().await;
    match fs::remove_file(app.data.uploads().join(&name)) {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(ApiError::not_found("No such upload.")),
        Err(e) => Err(e.into()),
    }
}

/// One upload as the dashboard lists it.
pub struct UploadEntry {
    /// The file name, which is also the last part of its URL.
    pub name: String,
    /// The size in bytes.
    pub size: u64,
    /// Whether any page's source mentions the upload's URL.
    pub used: bool,
}

impl UploadEntry {
    /// Returns the size for people: `512 B`, `14 KB`, `1.4 MB`.
    pub fn size_label(&self) -> String {
        match self.size {
            b if b < 1024 => format!("{b} B"),
            b if b < 1024 * 1024 => format!("{} KB", b.div_ceil(1024)),
            b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
        }
    }
}

/// Lists every upload, sorted by name, and marks the ones no page links to.
pub fn list(app: &App) -> io::Result<Vec<UploadEntry>> {
    let mut sources = String::new();
    let mut pages = vec![PagePath::home()];
    pages.extend(store::subtree(&app.data, &PagePath::home())?);
    for path in pages {
        if let Ok(source) = fs::read_to_string(app.data.page_file(&path)) {
            sources.push_str(&source);
            sources.push('\n');
        }
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(app.data.uploads())? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type()?.is_file() || !is_upload_name(&name) {
            continue;
        }
        let used = sources.contains(&format!("/_/uploads/{name}"));
        entries.push(UploadEntry {
            size: entry.metadata()?.len(),
            used,
            name,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_browser_file_names() {
        assert_eq!(clean_name("My Photo (1).JPG"), "my-photo-1.jpg");
        assert_eq!(clean_name("C:\\Users\\me\\report.pdf"), "report.pdf");
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name(".htaccess"), "file.htaccess");
        assert_eq!(clean_name("..."), "file");
        assert_eq!(clean_name("archive.tar.gz"), "archive-tar.gz");
        assert_eq!(clean_name("\u{e9}t\u{e9}.png"), "t.png");
    }

    #[test]
    fn keeps_cleaned_names_short() {
        assert_eq!(clean_name(&"a".repeat(300)).len(), 90);
        assert_eq!(
            clean_name(&format!("{}.{}", "a".repeat(300), "b".repeat(30))).len(),
            101
        );
    }

    #[test]
    fn adds_a_suffix_when_the_name_is_taken() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("photo.jpg"), "").unwrap();
        fs::write(dir.path().join("photo-1.jpg"), "").unwrap();
        fs::write(dir.path().join("notes"), "").unwrap();

        assert_eq!(free_name(dir.path(), "photo.jpg"), "photo-2.jpg");
        assert_eq!(free_name(dir.path(), "notes"), "notes-1");
        assert_eq!(free_name(dir.path(), "new.png"), "new.png");
    }

    #[test]
    fn labels_sizes_for_people() {
        let label = |size| {
            UploadEntry {
                name: String::new(),
                size,
                used: true,
            }
            .size_label()
        };
        assert_eq!(label(512), "512 B");
        assert_eq!(label(14 * 1024), "14 KB");
        assert_eq!(label(1_468_007), "1.4 MB");
    }

    #[test]
    fn accepts_only_cleaned_names_when_serving() {
        assert!(is_upload_name("photo-1.jpg"));
        for bad in ["", ".tmp", "a/b", "..", "A.png", "a b"] {
            assert!(!is_upload_name(bad), "{bad:?}");
        }
    }
}
