//! Reading: pages, groups, "not found", and the static files every page loads.

use std::fs;
use std::io;
use std::sync::{Arc, OnceLock};

use askama::Template;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;
use sha2::{Digest, Sha256};

use super::{Chrome, Here, render, server_error};
use crate::App;
use crate::page;
use crate::store::{Kind, PagePath};

#[derive(Embed)]
#[folder = "assets/"]
struct Assets;

#[derive(Template)]
#[template(path = "page.html")]
struct PageTemplate {
    chrome: Chrome,
    body: String,
    has_diagrams: bool,
}

#[derive(Template)]
#[template(path = "group.html")]
struct GroupTemplate {
    chrome: Chrome,
    owner: bool,
    path: String,
}

#[derive(Template)]
#[template(path = "not_found.html")]
struct NotFoundTemplate {
    chrome: Chrome,
}

/// `GET /`: the home page.
pub async fn home(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    show(app, &headers, PagePath::home()).await
}

/// `GET /{*path}`: a page, a group, or "not found".
///
/// A visitor gets the same "not found" for a protected page as for one that doesn't exist (R14).
pub async fn page(State(app): State<Arc<App>>, headers: HeaderMap, Path(path): Path<String>) -> Response {
    match PagePath::parse(&path) {
        Some(path) => show(app, &headers, path).await,
        None => not_found(&app, app.auth.is_owner(&headers)),
    }
}

async fn show(app: Arc<App>, headers: &HeaderMap, path: PagePath) -> Response {
    let owner = app.auth.is_owner(headers);
    let task_app = app.clone();
    match tokio::task::spawn_blocking(move || load(&task_app, &path, owner)).await {
        Ok(Ok(response)) => response,
        Ok(Err(e)) => server_error(e),
        Err(e) => server_error(e),
    }
}

fn load(app: &App, path: &PagePath, owner: bool) -> io::Result<Response> {
    if !owner && page::is_protected(&app.data, path)? {
        return Ok(not_found(app, owner));
    }
    let title = app
        .tree()
        .title(path)
        .map(str::to_owned)
        .unwrap_or_else(|| page::title_from_name(path.name().unwrap_or("home")));
    if let Some(body) = app.renderer.cached_body(&app.data, path)? {
        let template = PageTemplate {
            chrome: Chrome::new(app, title, Here::Page(path), owner),
            has_diagrams: body.contains("class=\"mermaid\""),
            body,
        };
        return Ok(render(StatusCode::OK, &template));
    }
    if app.data.kind(path) == Some(Kind::Group) {
        let template = GroupTemplate {
            chrome: Chrome::new(app, title, Here::Group(path), owner),
            owner,
            path: path.url(),
        };
        return Ok(render(StatusCode::OK, &template));
    }
    Ok(not_found(app, owner))
}

/// The response for a page that doesn't exist, or that a visitor may not see.
pub fn not_found(app: &App, owner: bool) -> Response {
    let template = NotFoundTemplate {
        chrome: Chrome::new(app, "Not found", Here::Other, owner),
    };
    render(StatusCode::NOT_FOUND, &template)
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

/// Returns a short hash of every embedded file, so the service worker's static cache changes whenever any of them
/// does.
pub fn asset_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let mut names: Vec<_> = Assets::iter().collect();
        names.sort();
        let mut hash = Sha256::new();
        for name in names {
            hash.update(name.as_bytes());
            if let Some(file) = Assets::get(&name) {
                hash.update(&file.data);
            }
        }
        hash.finalize().iter().take(6).map(|b| format!("{b:02x}")).collect()
    })
}

/// `GET /sw.js`: the service worker, served from the root so it controls every page (R04).
pub async fn service_worker() -> Response {
    let Some(file) = Assets::get("sw.js") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let script = String::from_utf8_lossy(&file.data).replace("__VERSION__", asset_version());
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        script,
    )
        .into_response()
}

/// `GET /manifest.webmanifest`: what the browser needs to install the wiki as an app (R04).
pub async fn manifest() -> Response {
    let manifest = serde_json::json!({
        "name": "me-wiki",
        "short_name": "me-wiki",
        "start_url": "/",
        "scope": "/",
        "display": "standalone",
        "background_color": "#fbfaf8",
        "theme_color": "#0b62c4",
        "icons": [
            { "src": "/_/static/icon-192.png", "sizes": "192x192", "type": "image/png" },
            { "src": "/_/static/icon-512.png", "sizes": "512x512", "type": "image/png" },
            { "src": "/_/static/icon-maskable-512.png", "sizes": "512x512", "type": "image/png", "purpose": "maskable" },
        ],
    });
    (
        [
            (header::CONTENT_TYPE, "application/manifest+json"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        manifest.to_string(),
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
