//! The HTTP routes and the pieces of HTML every page shares.

mod read;

use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

use crate::App;
use crate::store::{Kind, PagePath, Tree, TreeNode};

/// Builds the router for every route the app serves.
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/", get(read::home))
        .route("/{*path}", get(read::page))
        .route("/_/static/{*file}", get(read::static_file))
        .route("/_/custom.css", get(read::custom_css))
        .with_state(app)
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

/// Logs `error` and answers with a bare `500`.
fn server_error(error: impl std::fmt::Display) -> Response {
    tracing::error!("{error}");
    (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong.").into_response()
}

/// Renders the navigation tree as nested lists, marking `current` with `aria-current`.
fn tree_html(tree: &Tree, current: &PagePath) -> String {
    let mut html = String::new();
    write_nodes(&mut html, &tree.roots, current);
    html
}

fn write_nodes(html: &mut String, nodes: &[TreeNode], current: &PagePath) {
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
        if &node.path == current {
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
