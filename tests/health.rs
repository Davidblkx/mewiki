mod common;

use axum::http::StatusCode;
use common::Wiki;

#[tokio::test]
async fn answers_ok_when_the_data_folder_works() {
    let (response, body) = Wiki::with_pages(&[]).get("/_/health").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body, "ok");
}

#[tokio::test]
async fn answers_unavailable_when_the_data_folder_is_gone() {
    let wiki = Wiki::with_pages(&[]);
    std::fs::remove_dir_all(wiki.dir.path().join("pages")).unwrap();

    let (response, _) = wiki.get("/_/health").await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn probe_passes_against_a_running_server() {
    let wiki = Wiki::with_pages(&[]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, wiki.router()).into_future());

    let unspecified = std::net::SocketAddr::from(([0, 0, 0, 0], addr.port()));
    let result = tokio::task::spawn_blocking(move || mewiki::health::probe(unspecified))
        .await
        .unwrap();

    assert_eq!(result, Ok(()));
}

#[tokio::test]
async fn probe_fails_when_the_server_reports_a_problem() {
    let wiki = Wiki::with_pages(&[]);
    std::fs::remove_dir_all(wiki.dir.path().join("pages")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, wiki.router()).into_future());

    let result = tokio::task::spawn_blocking(move || mewiki::health::probe(addr))
        .await
        .unwrap();

    assert!(result.unwrap_err().contains("503"));
}

#[tokio::test]
async fn probe_fails_when_nothing_listens() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let result = tokio::task::spawn_blocking(move || mewiki::health::probe(addr))
        .await
        .unwrap();

    assert!(result.unwrap_err().contains("can't reach"));
}
