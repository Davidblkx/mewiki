//! The owner's screens: the editor for an existing page, the form for a new one, and the dashboard.

use std::fs;
use std::io;
use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;

use super::uploads::{self, UploadEntry};
use super::{Chrome, Here, read, render, server_error};
use crate::App;
use crate::page;
use crate::store::PagePath;

#[derive(Template)]
#[template(path = "editor.html")]
struct EditorTemplate {
    chrome: Chrome,
    creating: bool,
    path: String,
    parent: String,
    markdown: String,
    protected: bool,
    inherited: bool,
    cancel: String,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    chrome: Chrome,
    custom_css: String,
    uploads: Vec<UploadEntry>,
}

/// Which folder a new page goes in.
#[derive(Deserialize)]
pub struct NewQuery {
    parent: Option<String>,
}

/// `GET /_/edit` and `GET /_/edit/{*path}`: the editor for an existing page (R08).
pub async fn edit_page(State(app): State<Arc<App>>, path: Option<Path<String>>) -> Response {
    let Some(path) = PagePath::parse(path.as_ref().map_or("", |p| p.as_str())) else {
        return read::not_found(&app, true);
    };
    match load_editor(&app, &path) {
        Ok(Some(template)) => render(StatusCode::OK, &template),
        Ok(None) => read::not_found(&app, true),
        Err(e) => server_error(e),
    }
}

fn load_editor(app: &App, path: &PagePath) -> io::Result<Option<EditorTemplate>> {
    let source = match fs::read_to_string(app.data.page_file(path)) {
        Ok(source) => source,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let (front_matter, body) = page::split_front_matter(&source);
    let inherited = match path.ancestors().first() {
        Some(parent) => page::is_protected(&app.data, parent)?,
        None => false,
    };
    let title = page::title(body, path.name().unwrap_or("home"));
    Ok(Some(EditorTemplate {
        chrome: Chrome::new(app, format!("Editing {title}"), Here::Other, true),
        creating: false,
        path: path.url(),
        parent: String::new(),
        markdown: body.to_owned(),
        protected: front_matter.protected(),
        inherited,
        cancel: path.url(),
    }))
}

/// `GET /_/new?parent=`: the form for a new page under `parent`, which must already exist.
pub async fn new_page(State(app): State<Arc<App>>, Query(query): Query<NewQuery>) -> Response {
    let parent_url = query.parent.unwrap_or_else(|| "/".to_owned());
    let parent = parent_url.strip_prefix('/').and_then(PagePath::parse);
    let Some(parent) = parent.filter(|p| p.is_home() || app.data.kind(p).is_some()) else {
        return read::not_found(&app, true);
    };
    let inherited = if parent.is_home() {
        Ok(false)
    } else {
        page::is_protected(&app.data, &parent)
    };
    let inherited = match inherited {
        Ok(inherited) => inherited,
        Err(e) => return server_error(e),
    };
    let template = EditorTemplate {
        chrome: Chrome::new(&app, "New page", Here::Other, true),
        creating: true,
        path: String::new(),
        parent: parent.url(),
        markdown: String::new(),
        protected: false,
        inherited,
        cancel: parent.url(),
    };
    render(StatusCode::OK, &template)
}

/// `GET /_/dashboard`: custom CSS, Rebuild, and the list of uploads.
pub async fn dashboard(State(app): State<Arc<App>>) -> Response {
    let custom_css = fs::read_to_string(app.data.config().join("custom.css")).unwrap_or_default();
    let uploads = match uploads::list(&app) {
        Ok(uploads) => uploads,
        Err(e) => return server_error(e),
    };
    let template = DashboardTemplate {
        chrome: Chrome::new(&app, "Dashboard", Here::Other, true),
        custom_css,
        uploads,
    };
    render(StatusCode::OK, &template)
}
