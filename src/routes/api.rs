//! The owner's JSON API: preview, create and save, the page list, custom CSS and Rebuild.

use std::fs;
use std::io;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::App;
use crate::page;
use crate::store::{self, Kind, PagePath, TreeNode};

/// A failed API call: a status and a message the editor can show.
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        ApiError {
            status,
            message: message.into(),
        }
    }
}

impl From<io::Error> for ApiError {
    fn from(error: io::Error) -> Self {
        tracing::error!("{error}");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong on the server.")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({ "error": self.message }))).into_response()
    }
}

/// `POST /_/api/preview`: renders the Markdown in the body and returns the HTML fragment.
pub async fn preview(State(app): State<Arc<App>>, markdown: String) -> Response {
    let html = app.renderer.render(&markdown);
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        html,
    )
        .into_response()
}

/// The body of a save.
#[derive(Deserialize)]
pub struct SaveRequest {
    markdown: String,
    protected: bool,
}

/// The answer to a save: where the page now lives.
#[derive(Serialize)]
pub struct Saved {
    url: String,
}

/// `PUT /_/api/page` and `PUT /_/api/page/{*path}`: saves a page, or creates it when the request carries
/// `If-None-Match: *`.
///
/// A create fails with `412` when the page exists; a save fails with `404` when it doesn't. Both need an existing
/// parent, otherwise `409`, so the app never creates a group.
pub async fn save_page(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Option<Path<String>>,
    Json(request): Json<SaveRequest>,
) -> Result<Json<Saved>, ApiError> {
    let path = PagePath::parse(path.as_ref().map_or("", |p| p.as_str()))
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "That isn't a valid page name."))?;
    let creating = headers.get(header::IF_NONE_MATCH).is_some_and(|v| v == "*");
    let _write = app.writes.lock().await;
    let task_app = app.clone();
    tokio::task::spawn_blocking(move || write_page(&task_app, &path, &request, creating))
        .await
        .map_err(|e| io::Error::other(e.to_string()))?
}

fn write_page(app: &App, path: &PagePath, request: &SaveRequest, creating: bool) -> Result<Json<Saved>, ApiError> {
    let file = app.data.page_file(path);
    let existing = match fs::read_to_string(&file) {
        Ok(source) => Some(source),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    match (&existing, creating) {
        (Some(_), true) => {
            return Err(ApiError::new(
                StatusCode::PRECONDITION_FAILED,
                "A page with that name exists.",
            ));
        }
        (None, false) => return Err(ApiError::new(StatusCode::NOT_FOUND, "This page no longer exists.")),
        _ => {}
    }
    if let Some(parent) = path.ancestors().first()
        && app.data.kind(parent).is_none()
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "The page above this one doesn't exist.",
        ));
    }

    let mut front_matter = existing
        .as_deref()
        .map(|source| page::split_front_matter(source).0)
        .unwrap_or_default();
    front_matter.set_protected(request.protected);
    store::write_atomic(&file, front_matter.to_source(&request.markdown).as_bytes())?;
    let html = app.renderer.render(&request.markdown);
    store::write_atomic(&app.data.cached_body(path), html.as_bytes())?;
    app.rebuild_tree()?;
    Ok(Json(Saved { url: path.url() }))
}

/// One entry in the page list.
#[derive(Serialize)]
pub struct PageEntry {
    url: String,
    title: String,
    group: bool,
}

/// `GET /_/api/pages`: every page and group, home first, for Sync (R04) and the editor's link picker.
pub async fn pages(State(app): State<Arc<App>>) -> Json<Vec<PageEntry>> {
    let tree = app.tree();
    let mut entries = Vec::new();
    if let Some(title) = &tree.home_title {
        entries.push(PageEntry {
            url: "/".to_owned(),
            title: title.clone(),
            group: false,
        });
    }
    collect_entries(&tree.roots, &mut entries);
    Json(entries)
}

fn collect_entries(nodes: &[TreeNode], entries: &mut Vec<PageEntry>) {
    for node in nodes {
        entries.push(PageEntry {
            url: node.path.url(),
            title: node.title.clone(),
            group: node.kind == Kind::Group,
        });
        collect_entries(&node.children, entries);
    }
}

/// `PUT /_/api/custom-css`: replaces the custom CSS applied to every page (R02).
pub async fn save_custom_css(State(app): State<Arc<App>>, css: String) -> Result<StatusCode, ApiError> {
    let _write = app.writes.lock().await;
    store::write_atomic(&app.data.config().join("custom.css"), css.as_bytes())?;
    Ok(StatusCode::NO_CONTENT)
}

/// The answer to a rebuild.
#[derive(Serialize)]
pub struct Rebuilt {
    pages: usize,
}

/// `POST /_/api/rebuild`: deletes `cache/`, renders every page again, and rescans the tree.
pub async fn rebuild(State(app): State<Arc<App>>) -> Result<Json<Rebuilt>, ApiError> {
    let _write = app.writes.lock().await;
    let task_app = app.clone();
    let pages = tokio::task::spawn_blocking(move || rebuild_all(&task_app))
        .await
        .map_err(|e| io::Error::other(e.to_string()))??;
    Ok(Json(Rebuilt { pages }))
}

fn rebuild_all(app: &App) -> io::Result<usize> {
    match fs::remove_dir_all(app.data.cache()) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    app.data.init()?;
    app.rebuild_tree()?;
    let tree = app.tree();
    let mut paths = vec![PagePath::home()];
    collect_pages(&tree.roots, &mut paths);
    let mut rendered = 0;
    for path in paths {
        if app.renderer.cached_body(&app.data, &path)?.is_some() {
            rendered += 1;
        }
    }
    Ok(rendered)
}

fn collect_pages(nodes: &[TreeNode], paths: &mut Vec<PagePath>) {
    for node in nodes {
        if node.kind == Kind::Page {
            paths.push(node.path.clone());
        }
        collect_pages(&node.children, paths);
    }
}
