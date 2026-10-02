mod common;

use axum::http::StatusCode;
use common::Wiki;

fn wiki() -> Wiki {
    Wiki::with_pages(&[
        ("index.md", "# Welcome\n\nHello."),
        ("mushroom.md", "# Mushrooms\n\n| a | b |\n|---|---|\n| 1 | 2 |\n"),
        (
            "mushroom/chanterelle.md",
            "# Chanterelle\n\n```mermaid\ngraph TD; A-->B\n```\n",
        ),
        ("recipes/risotto.md", "# Risotto"),
    ])
}

#[tokio::test]
async fn creates_a_home_page_for_a_new_wiki_that_the_owner_can_edit() {
    let wiki = Wiki::with_pages(&[]);

    let (response, body) = wiki.owner_get("/").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(wiki.page_source("index.md"), "# MEWIKI\n");
    assert!(body.contains("<h1>MEWIKI</h1>"), "{body}");
    assert!(body.contains("href=\"/_/edit\">Edit</a>"), "{body}");
    assert!(body.contains("href=\"/_/new?parent=/\">New page</a>"), "{body}");
}

#[tokio::test]
async fn keeps_an_existing_home_page_at_startup() {
    let wiki = Wiki::with_pages(&[("index.md", "# Mine")]);

    assert_eq!(wiki.page_source("index.md"), "# Mine");
}

#[tokio::test]
async fn serves_the_home_page() {
    let (response, body) = wiki().get("/").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("<title>Welcome · me-wiki</title>"), "{body}");
    assert!(body.contains("<p>Hello.</p>"), "{body}");
}

#[tokio::test]
async fn serves_a_page_with_the_tree_and_marks_it_current() {
    let (response, body) = wiki().get("/mushroom").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("<table>"), "{body}");
    assert!(
        body.contains("<a href=\"/mushroom\" aria-current=\"page\">Mushrooms</a>"),
        "{body}"
    );
    assert!(
        body.contains("<a href=\"/mushroom/chanterelle\">Chanterelle</a>"),
        "{body}"
    );
    assert!(
        body.contains("<a href=\"/recipes\" class=\"group\">Recipes</a>"),
        "{body}"
    );
}

#[tokio::test]
async fn loads_mermaid_only_on_pages_with_diagrams() {
    let wiki = wiki();
    let (_, with) = wiki.get("/mushroom/chanterelle").await;
    let (_, without) = wiki.get("/mushroom").await;

    assert!(with.contains("mermaid.min.js"), "{with}");
    assert!(!without.contains("mermaid.min.js"), "{without}");
}

#[tokio::test]
async fn serves_a_group_as_a_blank_page() {
    let (response, body) = wiki().get("/recipes").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("<title>Recipes · me-wiki</title>"), "{body}");
    assert!(body.contains("<main></main>"), "{body}");
}

#[tokio::test]
async fn answers_not_found_for_missing_and_invalid_paths() {
    let wiki = wiki();
    for uri in [
        "/missing",
        "/mushroom/missing",
        "/Mushroom",
        "/index",
        "/mushroom/",
        "/..%2Fsecret",
        "/.hidden",
    ] {
        let (response, body) = wiki.get(uri).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        assert!(body.contains("<h1>Not found</h1>"), "{uri}");
    }
}

#[tokio::test]
async fn tells_the_browser_not_to_keep_page_html() {
    let wiki = wiki();
    for uri in ["/", "/mushroom", "/recipes", "/missing"] {
        let (response, _) = wiki.get(uri).await;
        assert_eq!(response.headers()["cache-control"], "no-store", "{uri}");
    }
}

#[tokio::test]
async fn picks_up_a_page_edited_outside_the_app() {
    let wiki = wiki();
    wiki.get("/mushroom").await;
    let file = wiki.dir.path().join("pages/mushroom.md");
    std::fs::write(&file, "# Fungi").unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(later)
        .unwrap();

    let (_, body) = wiki.get("/mushroom").await;

    assert!(body.contains("<h1>Fungi</h1>"), "{body}");
}

#[tokio::test]
async fn serves_embedded_static_files() {
    let (response, body) = wiki().get("/_/static/style.css").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/css");
    assert!(body.contains(".tree"));
}

#[tokio::test]
async fn answers_not_found_for_unknown_static_files() {
    let (response, _) = wiki().get("/_/static/nope.js").await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn serves_an_empty_custom_stylesheet_until_one_is_set() {
    let wiki = wiki();
    let (response, body) = wiki.get("/_/custom.css").await;
    assert_eq!(response.headers()["content-type"], "text/css; charset=utf-8");
    assert_eq!(body, "");

    std::fs::write(wiki.dir.path().join("config/custom.css"), "body { color: red; }").unwrap();
    let (_, body) = wiki.get("/_/custom.css").await;
    assert_eq!(body, "body { color: red; }");
}
