mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::Wiki;

fn wiki() -> Wiki {
    Wiki::with_pages(&[
        ("index.md", "# Home\n\n![photo](/_/uploads/photo.jpg)"),
        ("vault.md", "---\nprotected: true\n---\n# Vault"),
    ])
}

async fn upload(wiki: &Wiki, name: &str, body: impl Into<Body>) -> (StatusCode, serde_json::Value) {
    let uri = format!("/_/api/uploads?name={name}");
    let (response, body) = wiki.owner_send("POST", &uri, &[], body).await;
    (response.status(), serde_json::from_str(&body).unwrap_or_default())
}

fn uploaded(wiki: &Wiki, name: &str) -> Option<Vec<u8>> {
    std::fs::read(wiki.dir.path().join("uploads").join(name)).ok()
}

#[tokio::test]
async fn stores_an_upload_and_returns_markdown_for_it() {
    let wiki = wiki();
    let (status, body) = upload(&wiki, "My%20Photo.JPG", "jpeg bytes").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["url"], "/_/uploads/my-photo.jpg");
    assert_eq!(body["markdown"], "![my-photo.jpg](/_/uploads/my-photo.jpg)");
    assert_eq!(uploaded(&wiki, "my-photo.jpg").unwrap(), b"jpeg bytes");
}

#[tokio::test]
async fn links_non_images_instead_of_embedding_them() {
    let (_, body) = upload(&wiki(), "notes.pdf", "pdf").await;

    assert_eq!(body["markdown"], "[notes.pdf](/_/uploads/notes.pdf)");
}

#[tokio::test]
async fn adds_a_suffix_instead_of_overwriting() {
    let wiki = wiki();
    upload(&wiki, "photo.jpg", "first").await;
    let (_, body) = upload(&wiki, "photo.jpg", "second").await;

    assert_eq!(body["url"], "/_/uploads/photo-1.jpg");
    assert_eq!(uploaded(&wiki, "photo.jpg").unwrap(), b"first");
}

#[tokio::test]
async fn keeps_uploads_inside_the_uploads_folder() {
    let wiki = wiki();
    let (_, body) = upload(&wiki, "..%2F..%2Fpages%2Findex.md", "evil").await;

    assert_eq!(body["url"], "/_/uploads/index.md");
    assert_eq!(wiki.page_source("index.md"), "# Home\n\n![photo](/_/uploads/photo.jpg)");
}

#[tokio::test]
async fn accepts_two_megabytes_and_refuses_more() {
    let wiki = wiki();
    let (fits, _) = upload(&wiki, "big.bin", vec![0u8; 2 * 1024 * 1024]).await;
    let (too_big, _) = upload(&wiki, "huge.bin", vec![0u8; 2 * 1024 * 1024 + 1]).await;

    assert_eq!(fits, StatusCode::OK);
    assert_eq!(too_big, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(uploaded(&wiki, "huge.bin").is_none());
}

#[tokio::test]
async fn refuses_uploads_without_a_session() {
    let wiki = wiki();
    let request = Request::post("/_/api/uploads?name=x.png")
        .body(Body::from("x"))
        .unwrap();

    let (response, _) = wiki.send(request).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(uploaded(&wiki, "x.png").is_none());
}

#[tokio::test]
async fn serves_images_inline_and_sandboxed_to_anyone() {
    let wiki = wiki();
    upload(&wiki, "photo.png", "png").await;

    let (response, body) = wiki.get("/_/uploads/photo.png").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body, "png");
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["content-security-policy"], "sandbox");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(response.headers().get("content-disposition").is_none());
}

#[tokio::test]
async fn serves_svg_and_html_as_downloads() {
    let wiki = wiki();
    for name in ["drawing.svg", "page.html"] {
        upload(&wiki, name, "<script>alert(1)</script>").await;

        let (response, _) = wiki.get(&format!("/_/uploads/{name}")).await;

        assert_eq!(response.headers()["content-type"], "application/octet-stream", "{name}");
        assert_eq!(
            response.headers()["content-disposition"],
            format!("attachment; filename=\"{name}\""),
            "{name}"
        );
        assert_eq!(response.headers()["content-security-policy"], "sandbox", "{name}");
    }
}

#[tokio::test]
async fn answers_not_found_for_missing_or_odd_upload_names() {
    let wiki = wiki();
    for uri in [
        "/_/uploads/missing.png",
        "/_/uploads/..%2Fconfig%2Fsecret.key",
        "/_/uploads/.secret.tmp",
    ] {
        let (response, _) = wiki.get(uri).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn deletes_an_upload_even_when_a_page_uses_it() {
    let wiki = wiki();
    upload(&wiki, "photo.jpg", "jpeg").await;

    let (response, _) = wiki.owner_send("DELETE", "/_/api/uploads/photo.jpg", &[], "").await;
    let (missing, _) = wiki.owner_send("DELETE", "/_/api/uploads/photo.jpg", &[], "").await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert!(uploaded(&wiki, "photo.jpg").is_none());
}

#[tokio::test]
async fn keeps_uploads_when_their_page_is_deleted() {
    let wiki = Wiki::with_pages(&[("index.md", "# Home"), ("trip.md", "![a](/_/uploads/a.png)")]);
    upload(&wiki, "a.png", "png").await;

    wiki.owner_send("DELETE", "/_/api/page/trip", &[], "").await;

    assert!(uploaded(&wiki, "a.png").is_some());
}

#[tokio::test]
async fn lists_uploads_on_the_dashboard_and_marks_unused_ones() {
    let wiki = wiki();
    upload(&wiki, "photo.jpg", "used").await;
    upload(&wiki, "spare.png", "unused").await;

    let (_, body) = wiki.owner_get("/_/dashboard").await;

    assert!(
        body.contains("<a href=\"/_/uploads/photo.jpg\">photo.jpg</a>"),
        "{body}"
    );
    assert!(body.contains("4 B</span>"), "{body}");
    assert!(body.contains("6 B · unused</span>"), "{body}");
}

#[tokio::test]
async fn serves_uploads_used_by_protected_pages_to_anyone() {
    let wiki = Wiki::with_pages(&[("vault.md", "---\nprotected: true\n---\n![k](/_/uploads/key.png)")]);
    upload(&wiki, "key.png", "png").await;

    let (response, _) = wiki.get("/_/uploads/key.png").await;

    assert_eq!(response.status(), StatusCode::OK);
}
