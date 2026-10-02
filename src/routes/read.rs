//! Reading: pages, groups, "not found", and the static files every page loads.

use std::fs;
use std::io;
use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

use super::{page_response, server_error, tree_html};
use crate::App;
use crate::page;
use crate::store::{Kind, PagePath};

#[derive(Embed)]
#[folder = "assets/"]
struct Assets;

#[derive(Template)]
#[template(path = "page.html")]
struct PageTemplate {
    title: String,
    tree: String,
    body: String,
    has_diagrams: bool,
}

#[derive(Template)]
#[template(path = "group.html")]
struct GroupTemplate {
    title: String,
    tree: String,
}

#[derive(Template)]
#[template(path = "not_found.html")]
struct NotFoundTemplate {
    title: String,
    tree: String,
}

/// `GET /`: the home page.
pub async fn home(State(app): State<Arc<App>>) -> Response {
    show(app, PagePath::home()).await
}

/// `GET /{*path}`: a page, a group, or "not found".
pub async fn page(State(app): State<Arc<App>>, Path(path): Path<String>) -> Response {
    match PagePath::parse(&path) {
        Some(path) => show(app, path).await,
        None => not_found(&app),
    }
}

async fn show(app: Arc<App>, path: PagePath) -> Response {
    let task_app = app.clone();
    match tokio::task::spawn_blocking(move || load(&task_app, &path)).await {
        Ok(Ok(response)) => response,
        Ok(Err(e)) => server_error(e),
        Err(e) => server_error(e),
    }
}

fn load(app: &App, path: &PagePath) -> io::Result<Response> {
    let tree = app.tree();
    let title = || {
        tree.title(path)
            .map(str::to_owned)
            .unwrap_or_else(|| page::title_from_name(path.name().unwrap_or("home")))
    };
    if let Some(body) = app.renderer.cached_body(&app.data, path)? {
        let template = PageTemplate {
            title: title(),
            tree: tree_html(&tree, path),
            has_diagrams: body.contains("class=\"mermaid\""),
            body,
        };
        return Ok(render(StatusCode::OK, &template));
    }
    if app.data.kind(path) == Some(Kind::Group) {
        let template = GroupTemplate {
            title: title(),
            tree: tree_html(&tree, path),
        };
        return Ok(render(StatusCode::OK, &template));
    }
    Ok(not_found(app))
}

/// The response for a page that doesn't exist.
pub fn not_found(app: &App) -> Response {
    let template = NotFoundTemplate {
        title: "Not found".to_owned(),
        tree: tree_html(&app.tree(), &PagePath::home()),
    };
    render(StatusCode::NOT_FOUND, &template)
}

fn render(status: StatusCode, template: &impl Template) -> Response {
    match template.render() {
        Ok(html) => page_response(status, html),
        Err(e) => server_error(e),
    }
}

/// `GET /_/static/{*file}`: CSS, JavaScript and icons embedded in the binary.
pub async fn static_file(Path(file): Path<String>) -> Response {
    let Some(asset) = Assets::get(&file) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = asset.metadata.mimetype().to_owned();
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache".to_owned()),
        ],
        asset.data,
    )
        .into_response()
}

/// `GET /_/custom.css`: the owner's CSS from the dashboard, or an empty stylesheet when there is none (R02).
pub async fn custom_css(State(app): State<Arc<App>>) -> Response {
    let css = fs::read_to_string(app.data.config().join("custom.css")).unwrap_or_default();
    let mut response = css.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/css; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}
