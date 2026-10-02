mod common;

use axum::http::StatusCode;
use common::Wiki;

fn wiki() -> Wiki {
    Wiki::with_pages(&[
        (
            "index.md",
            "# Home\n\n[Mushrooms](/mushroom) and [Chanterelle](/mushroom/chanterelle#taste).",
        ),
        ("mushroom.md", "# Mushrooms\n\n[Self](/mushroom) `[code](/mushroom)`"),
        (
            "mushroom/chanterelle.md",
            "# Chanterelle\n\nBack to [mushrooms](/mushroom).",
        ),
        (
            "recipes/risotto.md",
            "# Risotto\n\nUses [chanterelle](/mushroom/chanterelle).",
        ),
        ("vault.md", "---\nprotected: true\n---\n# Vault"),
        ("vault/key.md", "# Key"),
        ("vault/own.md", "---\nprotected: true\n---\n# Own"),
        ("vault/key/inner.md", "# Inner"),
        ("open.md", "# Open"),
    ])
}

async fn move_page(wiki: &Wiki, from: &str, to: &str, make_public: bool) -> (StatusCode, serde_json::Value) {
    let body = serde_json::json!({ "from": from, "to": to, "make_public": make_public }).to_string();
    let (response, body) = wiki
        .owner_send("POST", "/_/api/move", &[("content-type", "application/json")], body)
        .await;
    (response.status(), serde_json::from_str(&body).unwrap_or_default())
}

async fn delete(wiki: &Wiki, uri: &str) -> (StatusCode, serde_json::Value) {
    let (response, body) = wiki.owner_send("DELETE", uri, &[], "").await;
    (response.status(), serde_json::from_str(&body).unwrap_or_default())
}

fn exists(wiki: &Wiki, file: &str) -> bool {
    wiki.dir.path().join("pages").join(file).exists()
}

#[tokio::test]
async fn renames_a_page_with_its_subpages_and_rewrites_links() {
    let wiki = wiki();
    let (status, body) = move_page(&wiki, "/mushroom", "/fungi", false).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["url"], "/fungi");
    assert!(exists(&wiki, "fungi.md") && exists(&wiki, "fungi/chanterelle.md"));
    assert!(!exists(&wiki, "mushroom.md") && !exists(&wiki, "mushroom"));
    assert_eq!(
        wiki.page_source("index.md"),
        "# Home\n\n[Mushrooms](/fungi) and [Chanterelle](/fungi/chanterelle#taste)."
    );
    assert_eq!(
        wiki.page_source("fungi.md"),
        "# Mushrooms\n\n[Self](/fungi) `[code](/mushroom)`"
    );
    assert_eq!(
        wiki.page_source("fungi/chanterelle.md"),
        "# Chanterelle\n\nBack to [mushrooms](/fungi)."
    );
    assert_eq!(
        wiki.page_source("recipes/risotto.md"),
        "# Risotto\n\nUses [chanterelle](/fungi/chanterelle)."
    );
}

#[tokio::test]
async fn serves_the_moved_page_at_its_new_address_with_rewritten_links() {
    let wiki = wiki();
    wiki.get("/").await;
    move_page(&wiki, "/mushroom", "/fungi", false).await;

    let (old, _) = wiki.get("/mushroom").await;
    let (new, _) = wiki.get("/fungi/chanterelle").await;
    let (_, home) = wiki.get("/").await;

    assert_eq!(old.status(), StatusCode::NOT_FOUND);
    assert_eq!(new.status(), StatusCode::OK);
    assert!(
        home.contains("<a href=\"/fungi/chanterelle#taste\">Chanterelle</a>"),
        "{home}"
    );
    assert!(home.contains("<a href=\"/fungi\">Mushrooms</a>"), "{home}");
}

#[tokio::test]
async fn moves_a_page_under_another_page_that_had_no_subpages() {
    let wiki = wiki();
    let (status, _) = move_page(&wiki, "/open", "/recipes/risotto/open", false).await;

    assert_eq!(status, StatusCode::OK);
    assert!(exists(&wiki, "recipes/risotto/open.md"));
}

#[tokio::test]
async fn moves_a_group() {
    let wiki = wiki();
    let (status, _) = move_page(&wiki, "/recipes", "/cooking", false).await;

    assert_eq!(status, StatusCode::OK);
    assert!(exists(&wiki, "cooking/risotto.md"));
}

#[tokio::test]
async fn refuses_a_move_into_the_pages_own_subpages() {
    let wiki = wiki();
    let (status, _) = move_page(&wiki, "/mushroom", "/mushroom/chanterelle/mushroom", false).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(exists(&wiki, "mushroom.md"));
}

#[tokio::test]
async fn refuses_a_move_under_a_missing_parent() {
    let wiki = wiki();
    let (status, _) = move_page(&wiki, "/open", "/missing/open", false).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(exists(&wiki, "open.md"));
    assert!(!exists(&wiki, "missing"));
}

#[tokio::test]
async fn refuses_a_move_onto_an_existing_page_or_group() {
    let wiki = wiki();
    for target in ["/open", "/recipes"] {
        let (status, _) = move_page(&wiki, "/mushroom", target, false).await;
        assert_eq!(status, StatusCode::CONFLICT, "{target}");
    }
}

#[tokio::test]
async fn refuses_to_move_the_home_page() {
    let wiki = wiki();
    let (status, _) = move_page(&wiki, "/", "/home", false).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(exists(&wiki, "index.md"));
}

#[tokio::test]
async fn asks_before_a_move_that_makes_pages_public() {
    let wiki = wiki();
    let (status, body) = move_page(&wiki, "/vault/key", "/key", false).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["confirm"], "make_public");
    assert_eq!(body["pages"], serde_json::json!(["/vault/key", "/vault/key/inner"]));
    assert!(exists(&wiki, "vault/key.md"));
}

#[tokio::test]
async fn makes_pages_public_when_the_move_says_so() {
    let wiki = wiki();
    let (status, _) = move_page(&wiki, "/vault/key", "/key", true).await;

    assert_eq!(status, StatusCode::OK);
    let (response, _) = wiki.get("/key/inner").await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn moves_pages_that_stay_protected_without_asking() {
    let wiki = wiki();
    let (own, _) = move_page(&wiki, "/vault/own", "/own", false).await;
    let (within, _) = move_page(&wiki, "/vault/key/inner", "/vault/inner", false).await;
    let (whole, _) = move_page(&wiki, "/vault", "/safe", false).await;

    assert_eq!(own, StatusCode::OK);
    assert_eq!(within, StatusCode::OK);
    assert_eq!(whole, StatusCode::OK);
}

#[tokio::test]
async fn deletes_a_page_without_subpages_and_leaves_links_to_it() {
    let wiki = wiki();
    let (status, body) = delete(&wiki, "/_/api/page/open").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["deleted"], serde_json::json!(["/open"]));
    assert!(!exists(&wiki, "open.md"));
    let (response, _) = wiki.get("/open").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn refuses_to_delete_a_page_with_subpages_without_the_flag() {
    let wiki = wiki();
    let (status, body) = delete(&wiki, "/_/api/page/vault").await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["confirm"], "subpages");
    assert_eq!(
        body["pages"],
        serde_json::json!(["/vault/key", "/vault/key/inner", "/vault/own"])
    );
    assert!(exists(&wiki, "vault.md") && exists(&wiki, "vault/key.md"));
}

#[tokio::test]
async fn deletes_the_whole_branch_with_the_flag() {
    let wiki = wiki();
    wiki.owner_get("/vault/key").await;
    let (status, body) = delete(&wiki, "/_/api/page/vault?subpages=true").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["deleted"].as_array().unwrap().len(), 4);
    assert!(!exists(&wiki, "vault.md") && !exists(&wiki, "vault"));
    assert!(!wiki.dir.path().join("cache/pages/vault").exists());
    let (_, home) = wiki.get("/").await;
    assert!(!home.contains("Vault"), "{home}");
}

#[tokio::test]
async fn deletes_a_group_with_its_pages() {
    let wiki = wiki();
    let (status, _) = delete(&wiki, "/_/api/page/recipes?subpages=true").await;

    assert_eq!(status, StatusCode::OK);
    assert!(!exists(&wiki, "recipes"));
}

#[tokio::test]
async fn refuses_to_delete_the_home_page() {
    let wiki = wiki();
    let (status, _) = delete(&wiki, "/_/api/page?subpages=true").await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(exists(&wiki, "index.md"));
}

#[tokio::test]
async fn answers_not_found_when_deleting_a_missing_page() {
    let (status, _) = delete(&wiki(), "/_/api/page/missing").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn refuses_moves_and_deletes_without_a_session() {
    let wiki = wiki();
    let move_request = axum::http::Request::post("/_/api/move")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(r#"{"from":"/open","to":"/x"}"#))
        .unwrap();
    let delete_request = axum::http::Request::delete("/_/api/page/open")
        .body(axum::body::Body::empty())
        .unwrap();

    let (moved, _) = wiki.send(move_request).await;
    let (deleted, _) = wiki.send(delete_request).await;

    assert_eq!(moved.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(deleted.status(), StatusCode::UNAUTHORIZED);
    assert!(exists(&wiki, "open.md"));
}

#[tokio::test]
async fn shows_move_and_delete_in_the_editor_but_not_for_home() {
    let wiki = wiki();
    let (_, page) = wiki.owner_get("/_/edit/mushroom").await;
    let (_, home) = wiki.owner_get("/_/edit").await;
    let (_, group) = wiki.owner_get("/recipes").await;
    let (_, visitor_group) = wiki.get("/recipes").await;

    assert!(page.contains("id=\"page-actions\" data-path=\"/mushroom\""), "{page}");
    assert!(!home.contains("id=\"page-actions\""), "{home}");
    assert!(group.contains("id=\"page-actions\" data-path=\"/recipes\""), "{group}");
    assert!(!visitor_group.contains("page-actions"), "{visitor_group}");
}
