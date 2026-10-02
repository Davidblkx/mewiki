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

/// The password every test wiki uses.
pub const PASSWORD: &str = "test-password";

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
        let app = App::new(data, PASSWORD, true).unwrap();
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

    /// Returns a valid session cookie, as the browser would send it.
    pub fn session(&self) -> String {
        let set_cookie = self.app.auth.new_session_cookie();
        set_cookie.split(';').next().unwrap().to_owned()
    }

    /// Sends a `GET` for `uri` as the logged-in owner.
    pub async fn owner_get(&self, uri: &str) -> (Response<()>, String) {
        let request = Request::get(uri)
            .header("cookie", self.session())
            .body(Body::empty())
            .unwrap();
        self.send(request).await
    }

    /// Sends `method` to `uri` as the owner, with `body` and any extra `headers`.
    pub async fn owner_send(
        &self,
        method: &str,
        uri: &str,
        headers: &[(&str, &str)],
        body: impl Into<Body>,
    ) -> (Response<()>, String) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", self.session());
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        self.send(request.body(body.into()).unwrap()).await
    }

    /// Saves or, with `create`, creates the page at `url` as the owner.
    pub async fn save(&self, url: &str, markdown: &str, protected: bool, create: bool) -> (Response<()>, String) {
        let body = serde_json::json!({ "markdown": markdown, "protected": protected }).to_string();
        let mut headers = vec![("content-type", "application/json")];
        if create {
            headers.push(("if-none-match", "*"));
        }
        let uri = if url == "/" {
            "/_/api/page".to_owned()
        } else {
            format!("/_/api/page{url}")
        };
        self.owner_send("PUT", &uri, &headers, body).await
    }

    /// Returns the contents of a file under `pages/`.
    pub fn page_source(&self, file: &str) -> String {
        fs::read_to_string(self.dir.path().join("pages").join(file)).unwrap()
    }
}
