mod common;

use axum::http::StatusCode;
use common::Wiki;

fn wiki() -> Wiki {
    Wiki::with_pages(&[("index.md", "# Home")])
}

#[tokio::test]
async fn serves_the_service_worker_from_the_root_with_the_asset_version() {
    let (response, body) = wiki().get("/sw.js").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/javascript; charset=utf-8");
    assert_eq!(response.headers()["cache-control"], "no-cache");
    assert!(!body.contains("__VERSION__"), "{body}");
    assert!(body.contains(&format!("const VERSION = \"{}\";", mewiki::routes::asset_version())));
}

#[tokio::test]
async fn serves_a_manifest_with_installable_icons() {
    let (response, body) = wiki().get("/manifest.webmanifest").await;
    let manifest: serde_json::Value = serde_json::from_str(&body).unwrap();

    assert_eq!(response.headers()["content-type"], "application/manifest+json");
    assert_eq!(manifest["start_url"], "/");
    assert_eq!(manifest["display"], "standalone");
    let sizes: Vec<&str> = manifest["icons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["sizes"].as_str().unwrap())
        .collect();
    assert_eq!(sizes, ["192x192", "512x512", "512x512"]);
}

#[tokio::test]
async fn serves_every_icon_the_manifest_names() {
    let wiki = wiki();
    let (_, body) = wiki.get("/manifest.webmanifest").await;
    let manifest: serde_json::Value = serde_json::from_str(&body).unwrap();

    for icon in manifest["icons"].as_array().unwrap() {
        let src = icon["src"].as_str().unwrap();
        let (response, _) = wiki.get(src).await;
        assert_eq!(response.status(), StatusCode::OK, "{src}");
        assert_eq!(response.headers()["content-type"], "image/png", "{src}");
    }
}

#[tokio::test]
async fn links_the_manifest_and_registers_the_worker_on_every_page() {
    let (_, body) = wiki().get("/").await;

    assert!(
        body.contains("<link rel=\"manifest\" href=\"/manifest.webmanifest\">"),
        "{body}"
    );
    assert!(body.contains("/_/static/register.js"), "{body}");
}

#[tokio::test]
async fn serves_the_offline_fallback_page() {
    let (response, body) = wiki().get("/_/static/offline.html").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("Not available offline"), "{body}");
}

#[tokio::test]
async fn offers_sync_on_the_dashboard() {
    let (_, body) = wiki().owner_get("/_/dashboard").await;

    assert!(body.contains("id=\"sync\""), "{body}");
    assert!(body.contains("/_/static/sync.js"), "{body}");
}
