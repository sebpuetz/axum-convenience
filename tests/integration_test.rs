use axum::routing::get;
use axum::Router;
use axum_convenience::{App, AppBuilder, ShutdownSignal};
use futures_util::FutureExt;
use hyper::{Body, Client, Response, StatusCode};
use once_cell::sync::Lazy;
use tokio::sync::oneshot;

static _TRACING_INIT: Lazy<()> = Lazy::new(|| {
    let env_filter = tracing_subscriber::EnvFilter::try_from_env("TRACE_TESTS")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt::SubscriberBuilder::default()
        .with_env_filter(env_filter)
        .init();
});

const HELLO_WORLD: &'static str = "hello world";

fn hello_world_app_builder() -> AppBuilder {
    *_TRACING_INIT;
    let router = Router::new().route("/hello-world", get(move || async move { HELLO_WORLD }));
    let addr = ([127, 0, 0, 1], 0).into();
    App::builder(addr, router)
}

async fn send_empty_get(uri: String) -> Response<Body> {
    let req = hyper::Request::get(uri)
        .body(Body::empty())
        .expect("failed to build request");
    let client = Client::new();
    client.request(req).await.expect("request failed")
}

#[tokio::test]
#[tracing::instrument]
async fn test_hello_world() {
    let app = hello_world_app_builder().spawn();
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
#[tracing::instrument]
async fn test_not_found() {
    let app = hello_world_app_builder().spawn();
    let addr = app.local_addr();
    let uri = format!("http://{}/not-hello-world", addr);

    let res = send_empty_get(uri).await;
    let status = res.status();

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[tracing::instrument]
async fn test_kill_ok() {
    let app = hello_world_app_builder().spawn();

    let res = app.kill().await;

    assert!(res.is_ok());
}

#[tokio::test]
#[tracing::instrument]
async fn test_shutdown_ok() {
    let builder = hello_world_app_builder();
    let (shutdown_tx, signal) = oneshot::channel::<()>();
    let shutdown = ShutdownSignal::Custom(signal.map(|_e| ()).boxed());
    let app = builder.with_graceful_shutdown(shutdown).spawn();

    let _ = shutdown_tx.send(());
    let res = app.await;

    assert!(res.is_ok());
}

#[tokio::test]
#[tracing::instrument]
async fn test_kill_after_shutdown_ok() {
    let builder = hello_world_app_builder();
    let (shutdown_tx, signal) = oneshot::channel::<()>();
    let shutdown = ShutdownSignal::Custom(signal.map(|_e| ()).boxed());
    let app = builder.with_graceful_shutdown(shutdown).spawn();

    let _ = shutdown_tx.send(());
    let res = app.kill().await;

    assert!(res.is_ok());
}

#[tokio::test]
#[tracing::instrument]
async fn test_provoke_server_panic() {
    let builder = hello_world_app_builder();
    let shutdown = ShutdownSignal::Custom(async { panic!("boom") }.boxed());
    let app = builder.with_graceful_shutdown(shutdown).spawn();

    let res = app.await;

    let e = res.expect_err("server should error");
    match e {
        axum_convenience::ServerError::PanicError(_err) => {}
        e => {
            panic!("expected the server task to panic: {:#?}", e)
        }
    }
}

#[tokio::test]
#[tracing::instrument]
async fn test_kill_doesnt_swallow_panic() {
    let builder = hello_world_app_builder();
    let (shutdown_tx, signal) = oneshot::channel::<()>();
    let (resp_chan, resp_recv) = oneshot::channel::<()>();
    let shutdown = ShutdownSignal::Custom(
        async move {
            let _ = signal.await;
            let _ = resp_chan.send(());
            panic!("boom")
        }
        .boxed(),
    );
    let app = builder.with_graceful_shutdown(shutdown).spawn();

    let _ = shutdown_tx.send(());
    // without awaiting the response from the shutdown handler we're aborting the task before the
    // handler panics.
    let _ = resp_recv.await;
    let res = app.kill().await;

    let e = res.expect_err("server should error");
    match e {
        axum_convenience::ServerError::PanicError(_err) => {}
        e => {
            panic!("expected the server task to panic: {:#?}", e)
        }
    }
}
