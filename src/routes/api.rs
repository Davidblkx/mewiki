//! The owner's JSON API: preview, create and save, the page list, custom CSS and Rebuild.

use std::fs;
use std::io;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::App;
use crate::page;
use crate::store::{self, Kind, PagePath, TreeNode};

/// A failed API call: a status and a message the editor can show.
///
/// When the call needs the owner to confirm first, `confirm` names what to confirm and `pages` lists the pages it
/// affects, so the editor can show them and repeat the call with the matching flag.
pub struct ApiError {
    status: StatusCode,
    message: String,
    confirm: Option<&'static str>,
    pages: Vec<String>,
}

impl ApiError {
    pub(super) fn new(status: StatusCode, message: impl Into<String>) -> Self {
        ApiError {
            status,
            message: message.into(),
            confirm: None,
            pages: Vec::new(),
        }
    }

    pub(super) fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    fn confirm(confirm: &'static str, message: impl Into<String>, pages: &[PagePath]) -> Self {
        ApiError {
            status: StatusCode::CONFLICT,
            message: message.into(),
            confirm: Some(confirm),
            pages: pages.iter().map(PagePath::url).collect(),
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
        let mut body = serde_json::json!({ "error": self.message });
        if let Some(confirm) = self.confirm {
            body["confirm"] = confirm.into();
            body["pages"] = self.pages.into();
        }
        (self.status, Json(body)).into_response()
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

fn parse_url(url: &str) -> Result<PagePath, ApiError> {
    url.strip_prefix('/')
        .and_then(PagePath::parse)
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, format!("{url} isn't a valid page address.")))
}

/// The body of a move.
#[derive(Deserialize)]
pub struct MoveRequest {
    from: String,
    to: String,
    #[serde(default)]
    make_public: bool,
}

/// `POST /_/api/move`: renames or moves a page or group, with its subpages, and rewrites every link to them (R06,
/// R07).
///
/// Refused when the target exists, is inside the page's own subpages, or has no parent, and for the home page. When
/// the move would make pages public, answers `409` with those pages unless the request says `make_public: true`.
pub async fn move_page(State(app): State<Arc<App>>, Json(request): Json<MoveRequest>) -> Result<Json<Saved>, ApiError> {
    let from = parse_url(&request.from)?;
    let to = parse_url(&request.to)?;
    let _write = app.writes.lock().await;
    let task_app = app.clone();
    tokio::task::spawn_blocking(move || move_branch(&task_app, &from, &to, request.make_public))
        .await
        .map_err(|e| io::Error::other(e.to_string()))?
}

fn move_branch(app: &App, from: &PagePath, to: &PagePath, make_public: bool) -> Result<Json<Saved>, ApiError> {
    let data = &app.data;
    if from.is_home() || to.is_home() {
        return Err(ApiError::new(StatusCode::CONFLICT, "The home page can't be moved."));
    }
    if data.kind(from).is_none() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "This page no longer exists."));
    }
    if to == from {
        return Ok(Json(Saved { url: to.url() }));
    }
    if to.starts_with(from) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "A page can't be moved inside its own subpages.",
        ));
    }
    if data.page_file(to).exists() || data.page_folder(to).exists() {
        return Err(ApiError::new(StatusCode::CONFLICT, format!("{to} already exists.")));
    }
    let to_parent = to.ancestors().into_iter().next();
    if let Some(parent) = &to_parent
        && data.kind(parent).is_none()
    {
        return Err(ApiError::new(StatusCode::CONFLICT, format!("{parent} doesn't exist.")));
    }

    let becoming_public = pages_becoming_public(app, from, to_parent.as_ref())?;
    if !becoming_public.is_empty() && !make_public {
        return Err(ApiError::confirm(
            "make_public",
            "Moving it there makes these pages public.",
            &becoming_public,
        ));
    }

    fs::create_dir_all(data.page_folder(to).parent().unwrap_or(&data.pages()))?;
    if data.page_file(from).exists() {
        fs::rename(data.page_file(from), data.page_file(to))?;
    }
    if data.page_folder(from).exists() {
        fs::rename(data.page_folder(from), data.page_folder(to))?;
    }
    remove_cached(app, from)?;
    rewrite_links_everywhere(app, from, to)?;
    app.rebuild_tree()?;
    Ok(Json(Saved { url: to.url() }))
}

/// Returns the pages in the branch at `from` that are protected now and wouldn't be under `to_parent`.
fn pages_becoming_public(app: &App, from: &PagePath, to_parent: Option<&PagePath>) -> io::Result<Vec<PagePath>> {
    let target_protected = match to_parent {
        Some(parent) => page::is_protected(&app.data, parent)?,
        None => false,
    };
    if target_protected {
        return Ok(Vec::new());
    }
    let mut branch = vec![from.clone()];
    branch.extend(store::subtree(&app.data, from)?);
    let mut becoming_public = Vec::new();
    for path in branch {
        if !page::is_protected(&app.data, &path)? {
            continue;
        }
        let mut protected_within_branch = false;
        let chain = std::iter::once(path.clone()).chain(path.ancestors());
        for candidate in chain.take_while(|p| p.starts_with(from)) {
            if own_protection(app, &candidate)? {
                protected_within_branch = true;
                break;
            }
        }
        if !protected_within_branch {
            becoming_public.push(path);
        }
    }
    Ok(becoming_public)
}

fn own_protection(app: &App, path: &PagePath) -> io::Result<bool> {
    match fs::read_to_string(app.data.page_file(path)) {
        Ok(source) => Ok(page::split_front_matter(&source).0.protected()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

fn rewrite_links_everywhere(app: &App, from: &PagePath, to: &PagePath) -> io::Result<()> {
    let mut pages = vec![PagePath::home()];
    pages.extend(store::subtree(&app.data, &PagePath::home())?);
    for path in pages {
        let file = app.data.page_file(&path);
        let source = match fs::read_to_string(&file) {
            Ok(source) => source,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        if let Some(rewritten) = page::rewrite_links(&source, from, to) {
            store::write_atomic(&file, rewritten.as_bytes())?;
        }
    }
    Ok(())
}

fn remove_cached(app: &App, path: &PagePath) -> io::Result<()> {
    let body = app.data.cached_body(path);
    for result in [fs::remove_file(&body), fs::remove_dir_all(body.with_extension(""))] {
        match result {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Whether a delete may take the page's subpages with it.
#[derive(Deserialize)]
pub struct DeleteQuery {
    #[serde(default)]
    subpages: bool,
}

/// The answer to a delete: every page that went.
#[derive(Serialize)]
pub struct Deleted {
    deleted: Vec<String>,
}

/// `DELETE /_/api/page/{*path}`: deletes a page or group and everything under it (R06).
///
/// Answers `409` with the list of subpages unless the request says `?subpages=true`, so a script or a stale tab
/// can't remove a branch by accident. The home page can't be deleted. Uploads stay (R10).
pub async fn delete_page(
    State(app): State<Arc<App>>,
    path: Option<Path<String>>,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<Deleted>, ApiError> {
    let path = PagePath::parse(path.as_ref().map_or("", |p| p.as_str()))
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "That isn't a valid page name."))?;
    let _write = app.writes.lock().await;
    let task_app = app.clone();
    tokio::task::spawn_blocking(move || delete_branch(&task_app, &path, query.subpages))
        .await
        .map_err(|e| io::Error::other(e.to_string()))?
}

fn delete_branch(app: &App, path: &PagePath, with_subpages: bool) -> Result<Json<Deleted>, ApiError> {
    if path.is_home() {
        return Err(ApiError::new(StatusCode::CONFLICT, "The home page can't be deleted."));
    }
    if app.data.kind(path).is_none() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "This page no longer exists."));
    }
    let subpages = store::subtree(&app.data, path)?;
    if !subpages.is_empty() && !with_subpages {
        return Err(ApiError::confirm(
            "subpages",
            "Deleting this page also deletes its subpages.",
            &subpages,
        ));
    }
    match fs::remove_file(app.data.page_file(path)) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    match fs::remove_dir_all(app.data.page_folder(path)) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    remove_cached(app, path)?;
    app.rebuild_tree()?;
    let mut deleted = vec![path.url()];
    deleted.extend(subpages.iter().map(PagePath::url));
    Ok(Json(Deleted { deleted }))
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
