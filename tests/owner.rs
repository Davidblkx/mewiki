mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{PASSWORD, Wiki};

fn wiki() -> Wiki {
    Wiki::with_pages(&[
        ("index.md", "# Welcome"),
        ("mushroom.md", "---\ntags: fungi\n---\n# Mushrooms"),
        ("secret.md", "---\nprotected: true\n---\n# Secret"),
        ("secret/inner.md", "# Inner"),
        ("recipes/risotto.md", "# Risotto"),
    ])
}

fn login_request(password: &str, next: &str) -> Request<Body> {
    Request::post("/_/login")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!("password={password}&next={next}")))
        .unwrap()
}

#[tokio::test]
async fn logs_in_with_the_password_and_returns_to_next() {
    let wiki = wiki();
    let (response, _) = wiki.send(login_request(PASSWORD, "%2Fsecret")).await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()["location"], "/secret");
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.starts_with("mewiki_session="), "{cookie}");
    assert!(cookie.contains("HttpOnly; SameSite=Strict; Secure"), "{cookie}");
}

#[tokio::test]
async fn refuses_a_wrong_password_after_a_delay() {
    let wiki = wiki();
    let started = std::time::Instant::now();
    let (response, body) = wiki.send(login_request("wrong", "%2F")).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().get("set-cookie").is_none());
    assert!(body.contains("That password isn"), "{body}");
    assert!(started.elapsed() >= std::time::Duration::from_millis(900));
}

#[tokio::test]
async fn never_sends_the_owner_to_another_site_after_login() {
    let (response, _) = wiki().send(login_request(PASSWORD, "%2F%2Fevil.example")).await;

    assert_eq!(response.headers()["location"], "/");
}

#[tokio::test]
async fn logs_out_by_clearing_the_cookie() {
    let (response, _) = wiki()
        .send(Request::post("/_/logout").body(Body::empty()).unwrap())
        .await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(response.headers()["set-cookie"].to_str().unwrap().contains("Max-Age=0"));
}

#[tokio::test]
async fn hides_protected_pages_from_visitors_exactly_like_missing_ones() {
    let wiki = wiki();
    let (missing, missing_body) = wiki.get("/nothing-here").await;
    for uri in ["/secret", "/secret/inner"] {
        let (response, body) = wiki.get(uri).await;
        assert_eq!(response.status(), missing.status(), "{uri}");
        assert_eq!(body, missing_body, "{uri}");
    }
}

#[tokio::test]
async fn shows_protected_pages_to_the_owner() {
    let wiki = wiki();
    let (response, body) = wiki.owner_get("/secret/inner").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("<h1>Inner</h1>"), "{body}");
}

#[tokio::test]
async fn hides_a_page_protected_by_editing_its_file_on_disk() {
    let wiki = wiki();
    let (before, _) = wiki.get("/mushroom").await;
    std::fs::write(
        wiki.dir.path().join("pages/mushroom.md"),
        "---\nprotected: yes\n---\n# Mushrooms",
    )
    .unwrap();

    let (after, _) = wiki.get("/mushroom").await;

    assert_eq!(before.status(), StatusCode::OK);
    assert_eq!(after.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn lists_protected_titles_in_the_tree_for_everyone() {
    let (_, body) = wiki().get("/").await;

    assert!(body.contains(">Secret</a><span class=\"lock\""), "{body}");
}

#[tokio::test]
async fn sends_visitors_from_owner_pages_to_login() {
    let wiki = wiki();
    for (uri, next) in [
        ("/_/edit/mushroom", "/_/login?next=/_/edit/mushroom"),
        ("/_/new?parent=/", "/_/login?next=/_/new%3Fparent%3D/"),
        ("/_/dashboard", "/_/login?next=/_/dashboard"),
    ] {
        let (response, _) = wiki.get(uri).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(response.headers()["location"], next, "{uri}");
    }
}

#[tokio::test]
async fn refuses_owner_api_calls_without_a_session() {
    let wiki = wiki();
    let requests = [
        Request::post("/_/api/preview").body(Body::from("# x")).unwrap(),
        Request::put("/_/api/page/mushroom")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"markdown":"x","protected":false}"#))
            .unwrap(),
        Request::get("/_/api/pages").body(Body::empty()).unwrap(),
        Request::put("/_/api/custom-css").body(Body::from("body{}")).unwrap(),
        Request::post("/_/api/rebuild").body(Body::empty()).unwrap(),
    ];
    for request in requests {
        let uri = request.uri().to_string();
        let (response, _) = wiki.send(request).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
    }
    assert_eq!(wiki.page_source("mushroom.md"), "---\ntags: fungi\n---\n# Mushrooms");
}

#[tokio::test]
async fn refuses_a_forged_session_cookie() {
    let wiki = wiki();
    let request = Request::get("/secret")
        .header("cookie", "mewiki_session=9999999999.deadbeef")
        .body(Body::empty())
        .unwrap();

    let (response, _) = wiki.send(request).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn shows_owner_links_only_when_logged_in() {
    let wiki = wiki();
    let (_, visitor) = wiki.get("/mushroom").await;
    let (_, owner) = wiki.owner_get("/mushroom").await;

    assert!(visitor.contains("Log in"), "{visitor}");
    assert!(!visitor.contains("/_/edit"), "{visitor}");
    assert!(owner.contains("href=\"/_/edit/mushroom\""), "{owner}");
    assert!(owner.contains("href=\"/_/new?parent=/mushroom\""), "{owner}");
}

#[tokio::test]
async fn opens_the_editor_with_the_body_and_without_front_matter() {
    let (response, body) = wiki().owner_get("/_/edit/mushroom").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("># Mushrooms</textarea>"), "{body}");
    assert!(!body.contains("tags: fungi"), "{body}");
}

#[tokio::test]
async fn opens_the_editor_for_the_home_page() {
    let (response, body) = wiki().owner_get("/_/edit").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("># Welcome</textarea>"), "{body}");
}

#[tokio::test]
async fn notes_protection_inherited_from_above_in_the_editor() {
    let wiki = wiki();
    let (_, inner) = wiki.owner_get("/_/edit/secret/inner").await;
    let (_, top) = wiki.owner_get("/_/edit/mushroom").await;

    assert!(inner.contains("A page above this one is protected"), "{inner}");
    assert!(!top.contains("A page above this one is protected"), "{top}");
}

#[tokio::test]
async fn opens_the_new_page_form_only_under_an_existing_parent() {
    let wiki = wiki();
    for parent in ["/", "/mushroom", "/recipes"] {
        let (response, _) = wiki.owner_get(&format!("/_/new?parent={parent}")).await;
        assert_eq!(response.status(), StatusCode::OK, "{parent}");
    }
    let (response, _) = wiki.owner_get("/_/new?parent=/missing").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn saves_a_page_keeping_unknown_front_matter_keys() {
    let wiki = wiki();
    let (response, body) = wiki.save("/mushroom", "# Fungi", true, false).await;

    assert_eq!(response.status(), StatusCode::OK, "{body}");
    assert_eq!(body, r#"{"url":"/mushroom"}"#);
    assert_eq!(
        wiki.page_source("mushroom.md"),
        "---\ntags: fungi\nprotected: true\n---\n# Fungi"
    );
    let (_, page) = wiki.owner_get("/mushroom").await;
    assert!(page.contains("<h1>Fungi</h1>"), "{page}");
}

#[tokio::test]
async fn unprotects_a_page_by_removing_the_key() {
    let wiki = wiki();
    wiki.save("/secret", "# Secret", false, false).await;

    assert_eq!(wiki.page_source("secret.md"), "# Secret");
    let (response, _) = wiki.get("/secret").await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn creates_a_page_and_adds_it_to_the_tree() {
    let wiki = wiki();
    let (response, _) = wiki.save("/mushroom/morel", "# Morel", false, true).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(wiki.page_source("mushroom/morel.md"), "# Morel");
    let (_, home) = wiki.get("/").await;
    assert!(home.contains("<a href=\"/mushroom/morel\">Morel</a>"), "{home}");
}

#[tokio::test]
async fn creates_a_page_under_a_group() {
    let (response, _) = wiki().save("/recipes/soup", "# Soup", false, true).await;

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn refuses_to_create_over_an_existing_page() {
    let wiki = wiki();
    let (response, body) = wiki.save("/mushroom", "# Replaced", false, true).await;

    assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    assert!(body.contains("exists"), "{body}");
    assert_eq!(wiki.page_source("mushroom.md"), "---\ntags: fungi\n---\n# Mushrooms");
}

#[tokio::test]
async fn refuses_to_save_a_page_that_no_longer_exists() {
    let wiki = wiki();
    let (response, _) = wiki.save("/gone", "# Gone", false, false).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(!wiki.dir.path().join("pages/gone.md").exists());
}

#[tokio::test]
async fn refuses_to_create_a_page_under_a_missing_parent() {
    let wiki = wiki();
    let (response, _) = wiki.save("/missing/child", "# Child", false, true).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(!wiki.dir.path().join("pages/missing").exists());
}

#[tokio::test]
async fn refuses_invalid_page_names() {
    let (response, _) = wiki().save("/Bad_Name", "# x", false, true).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn saves_the_home_page() {
    let wiki = wiki();
    let (response, _) = wiki.save("/", "# New home", false, false).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(wiki.page_source("index.md"), "# New home");
}

#[tokio::test]
async fn previews_markdown_as_html() {
    let (response, body) = wiki()
        .owner_send("POST", "/_/api/preview", &[], "| a |\n|---|\n| 1 |")
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("<table>"), "{body}");
}

#[tokio::test]
async fn lists_every_page_and_group_for_the_picker() {
    let (_, body) = wiki().owner_get("/_/api/pages").await;
    let pages: serde_json::Value = serde_json::from_str(&body).unwrap();
    let urls: Vec<&str> = pages
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["url"].as_str().unwrap())
        .collect();

    assert_eq!(
        urls,
        [
            "/",
            "/mushroom",
            "/recipes",
            "/recipes/risotto",
            "/secret",
            "/secret/inner"
        ]
    );
    assert_eq!(pages[2]["group"], true);
}

#[tokio::test]
async fn saves_custom_css() {
    let wiki = wiki();
    let (response, _) = wiki
        .owner_send("PUT", "/_/api/custom-css", &[], "main { color: red; }")
        .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let (_, css) = wiki.get("/_/custom.css").await;
    assert_eq!(css, "main { color: red; }");
}

#[tokio::test]
async fn rebuild_renders_every_page_again() {
    let wiki = wiki();
    wiki.get("/mushroom").await;
    std::fs::write(wiki.dir.path().join("cache/pages/mushroom.html"), "<p>stale</p>").unwrap();

    let (response, body) = wiki.owner_send("POST", "/_/api/rebuild", &[], "").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body, r#"{"pages":5}"#);
    let (_, page) = wiki.get("/mushroom").await;
    assert!(page.contains("<h1>Mushrooms</h1>"), "{page}");
}

#[tokio::test]
async fn rebuild_picks_up_pages_added_outside_the_app() {
    let wiki = wiki();
    std::fs::write(wiki.dir.path().join("pages/added.md"), "# Added").unwrap();

    wiki.owner_send("POST", "/_/api/rebuild", &[], "").await;

    let (_, home) = wiki.get("/").await;
    assert!(home.contains("<a href=\"/added\">Added</a>"), "{home}");
}
