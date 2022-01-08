use hyper::StatusCode;

mod helpers;
use helpers::*;

#[tokio::test]
async fn test_hello_world() {
    let app = hello_world_app_builder()
        .spawn()
        .await
        .expect("failed building app");
    let addr = app.local_addr();
    let uri = format!("http://{}/hello-world", addr);

    let res = send_empty_get(uri).await;
    let status = res.status();
    let body = hyper::body::to_bytes(res.into_body())
        .await
        .expect("failed to collect body");
    let text = std::str::from_utf8(&*body).expect("failed to decode response text");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(text, HELLO_WORLD);
}

#[tokio::test]
async fn test_not_found() {
    let app = hello_world_app_builder()
        .spawn()
        .await
        .expect("failed building app");
    let addr = app.local_addr();
    let uri = format!("http://{}/not-hello-world", addr);

    let res = send_empty_get(uri).await;
    let status = res.status();

    assert_eq!(status, StatusCode::NOT_FOUND);
}
