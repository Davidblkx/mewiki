//! Helpers shared by the integration tests: a temporary data folder and requests against the full router.

#![allow(dead_code)]

use std::fs;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response};
use http_body_util::BodyExt;
use mewiki::store::DataDir;
use mewiki::{App, routes};
use tower::ServiceExt;

/// A running app over a temporary data folder that is deleted when the wiki is dropped.
pub struct Wiki {
    pub dir: tempfile::TempDir,
    pub app: Arc<App>,
}

impl Wiki {
    /// Creates a wiki whose `pages/` holds `files`, given as paths relative to `pages/` and their contents.
    pub fn with_pages(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir::new(dir.path());
        data.init().unwrap();
        for (file, contents) in files {
            let path = data.pages().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let app = App::new(data).unwrap();
        Wiki { dir, app }
    }

    /// Returns the router serving this wiki.
    pub fn router(&self) -> Router {
        routes::router(self.app.clone())
    }

    /// Sends `request` and returns the response with its body read into a string.
    pub async fn send(&self, request: Request<Body>) -> (Response<()>, String) {
        let response = self.router().oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.unwrap().to_bytes();
        (
            Response::from_parts(parts, ()),
            String::from_utf8_lossy(&bytes).into_owned(),
        )
    }

    /// Sends a `GET` for `uri`.
    pub async fn get(&self, uri: &str) -> (Response<()>, String) {
        self.send(Request::get(uri).body(Body::empty()).unwrap()).await
    }
}
