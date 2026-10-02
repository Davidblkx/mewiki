//! The HTTP routes and the pieces of HTML every page shares.

mod api;
mod edit;
mod read;
mod session;
mod uploads;

pub use read::asset_version;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post, put};

use crate::App;
use crate::auth::{self, percent_encode};
use crate::store::{Kind, PagePath, Tree, TreeNode};

/// Builds the router for every route the app serves.
pub fn router(app: Arc<App>) -> Router {
    let owner_pages = Router::new()
        .route("/_/edit", get(edit::edit_page))
        .route("/_/edit/{*path}", get(edit::edit_page))
        .route("/_/new", get(edit::new_page))
        .route("/_/dashboard", get(edit::dashboard))
        .route_layer(middleware::from_fn_with_state(app.clone(), auth::require_owner_page));
    let owner_api = Router::new()
        .route("/_/api/preview", post(api::preview))
        .route("/_/api/page", put(api::save_page).delete(api::delete_page))
        .route("/_/api/page/{*path}", put(api::save_page).delete(api::delete_page))
        .route("/_/api/move", post(api::move_page))
        .route("/_/api/pages", get(api::pages))
        .route("/_/api/custom-css", put(api::save_custom_css))
        .route("/_/api/rebuild", post(api::rebuild))
        .route(
            "/_/api/uploads",
            post(uploads::upload).layer(DefaultBodyLimit::max(uploads::MAX_UPLOAD_BYTES)),
        )
        .route("/_/api/uploads/{name}", delete(uploads::delete))
        .route_layer(middleware::from_fn_with_state(app.clone(), auth::require_owner_api));
    Router::new()
        .route("/", get(read::home))
        .route("/{*path}", get(read::page))
        .route("/_/static/{*file}", get(read::static_file))
        .route("/sw.js", get(read::service_worker))
        .route("/manifest.webmanifest", get(read::manifest))
        .route("/_/custom.css", get(read::custom_css))
        .route("/_/uploads/{name}", get(uploads::serve))
        .route("/_/login", get(session::login_page).post(session::login))
        .route("/_/logout", post(session::logout))
        .merge(owner_pages)
        .merge(owner_api)
        .with_state(app)
}

/// The parts of the layout every template shares.
pub struct Chrome {
    /// The page title, shown in the browser tab.
    pub title: String,
    /// The navigation tree as HTML.
    pub tree: String,
    /// The header links as HTML: edit and dashboard links for the owner, a login link for visitors.
    pub actions: String,
}

/// What the current screen shows, which decides the header links.
pub enum Here<'a> {
    /// A page, including the home page.
    Page(&'a PagePath),
    /// A group.
    Group(&'a PagePath),
    /// Anything else: the editor, the dashboard, login, "not found".
    Other,
}

impl Chrome {
    /// Builds the shared layout parts for a screen titled `title`.
    pub fn new(app: &App, title: impl Into<String>, here: Here<'_>, owner: bool) -> Self {
        let current = match here {
            Here::Page(path) | Here::Group(path) => Some(path),
            Here::Other => None,
        };
        Chrome {
            title: title.into(),
            tree: tree_html(&app.tree(), current),
            actions: actions_html(&here, owner),
        }
    }
}

fn actions_html(here: &Here<'_>, owner: bool) -> String {
    if !owner {
        let next = match here {
            Here::Page(path) | Here::Group(path) => path.url(),
            Here::Other => "/".to_owned(),
        };
        return format!(
            "<a href=\"/_/login?next={}\">Log in</a>",
            escape(&percent_encode(&next))
        );
    }
    let mut html = String::new();
    if let Here::Page(path) = here {
        let edit = if path.is_home() {
            "/_/edit".to_owned()
        } else {
            format!("/_/edit{}", path.url())
        };
        html.push_str(&format!("<a href=\"{}\">Edit</a>", escape(&edit)));
    }
    if let Here::Page(path) | Here::Group(path) = here {
        html.push_str(&format!(
            "<a href=\"/_/new?parent={}\">New page</a>",
            escape(&percent_encode(&path.url()))
        ));
    }
    html.push_str("<a href=\"/_/dashboard\">Dashboard</a>");
    html.push_str("<form method=\"post\" action=\"/_/logout\"><button type=\"submit\">Log out</button></form>");
    html
}

/// Wraps rendered HTML in a response the browser must not keep in its own cache.
///
/// Page HTML can come from a logged-in session, so the browser keeps nothing; offline copies come only from the
/// service worker, which this header doesn't affect.
fn page_response(status: StatusCode, html: String) -> Response {
    let mut response = (status, Html(html)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Renders `template` into a [`page_response`], or a `500` when rendering fails.
fn render(status: StatusCode, template: &impl askama::Template) -> Response {
    match template.render() {
        Ok(html) => page_response(status, html),
        Err(e) => server_error(e),
    }
}

/// Logs `error` and answers with a bare `500`.
fn server_error(error: impl std::fmt::Display) -> Response {
    tracing::error!("{error}");
    (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong.").into_response()
}

/// Renders the navigation tree as nested lists, marking `current` with `aria-current`.
fn tree_html(tree: &Tree, current: Option<&PagePath>) -> String {
    let mut html = String::new();
    write_nodes(&mut html, &tree.roots, current);
    html
}

fn write_nodes(html: &mut String, nodes: &[TreeNode], current: Option<&PagePath>) {
    if nodes.is_empty() {
        return;
    }
    html.push_str("<ul>");
    for node in nodes {
        html.push_str("<li><a href=\"");
        html.push_str(&escape(&node.path.url()));
        html.push('"');
        if node.kind == Kind::Group {
            html.push_str(" class=\"group\"");
        }
        if Some(&node.path) == current {
            html.push_str(" aria-current=\"page\"");
        }
        html.push('>');
        html.push_str(&escape(&node.title));
        html.push_str("</a>");
        if node.protected {
            html.push_str("<span class=\"lock\" title=\"Protected\">&#128274;</span>");
        }
        write_nodes(html, &node.children, current);
        html.push_str("</li>");
    }
    html.push_str("</ul>");
}

/// Escapes text for use in HTML content and quoted attributes.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
