mod helpers;
use std::time::Duration;

use axum_convenience::ShutdownSignal;
use futures_util::FutureExt;
use helpers::*;
use hyper::StatusCode;
use tokio::sync::oneshot;

#[tokio::test]
async fn test_kill_ok() {
    let app = hello_world_app_builder()
        .spawn()
        .await
        .expect("failed building app");

    let res = app.shutdown().await;

    assert!(res.is_ok());
}

#[tokio::test]
async fn test_graceful_shutdown_signal_ok() {
    let builder = hello_world_app_builder();
    let (shutdown_tx, signal) = oneshot::channel::<()>();
    let shutdown = ShutdownSignal::custom(signal.map(|_e| ()).boxed());
    let app = builder
        .with_graceful_shutdown(shutdown)
        .spawn()
        .await
        .expect("failed building app");

    let _ = shutdown_tx.send(());
    let res = app.await;

    assert!(res.is_ok());
}

#[tokio::test]
async fn test_kill_after_shutdown_ok() {
    let builder = hello_world_app_builder();
    let (shutdown_tx, signal) = oneshot::channel::<()>();
    let shutdown = ShutdownSignal::custom(signal.map(|_e| ()).boxed());
    let app = builder
        .with_graceful_shutdown(shutdown)
        .spawn()
        .await
        .expect("failed building app");

    let _ = shutdown_tx.send(());
    let res = app.shutdown().await;

    assert!(res.is_ok());
}

#[tokio::test]
async fn test_provoke_server_panic() {
    let builder = hello_world_app_builder();
    let (shutdown_tx, signal) = oneshot::channel::<()>();
    let shutdown = ShutdownSignal::custom(
        async {
            // wait for signal to ensure we're even binding to the port
            let _ = signal.await;
            panic!("boom")
        }
        .boxed(),
    );
    let app = builder
        .with_graceful_shutdown(shutdown)
        .spawn()
        .await
        .expect("failed building app");

    let _ = shutdown_tx.send(());

    match app.await.expect_err("expected app to error") {
        axum_convenience::ServerError::PanicError(_err) => {}
        e => {
            panic!("expected the server task to panic: {:#?}", e)
        }
    }
}

#[tokio::test]
async fn test_kill_doesnt_swallow_panic() {
    let builder = hello_world_app_builder();
    let (shutdown_tx, signal) = oneshot::channel::<()>();
    let shutdown = ShutdownSignal::custom(
        async move {
            let _ = signal.await;
            panic!("boom")
        }
        .boxed(),
    );
    let app = builder
        .with_graceful_shutdown(shutdown)
        .spawn()
        .await
        .expect("failed building app");

    let _ = shutdown_tx.send(());
    let res = app.shutdown().await;

    let e = res.expect_err("server should error");
    match e {
        axum_convenience::ServerError::PanicError(_err) => {}
        e => {
            panic!("expected the server task to panic: {:#?}", e)
        }
    }
}

#[tokio::test]
async fn test_explicit_graceful_shutdown_completes_request() {
    let app = byte_handler_app_builder()
        .spawn()
        .await
        .expect("failed building app");
    let addr = app.local_addr();
    let uri = format!("http://{}/", addr);

    let request = tokio::spawn(half_sec_post_request(uri));
    // leave some time for the request to start
    tokio::time::sleep(Duration::from_millis(100)).await;
    app.graceful_shutdown(None)
        .await
        .expect("server should exit without error");

    let req = request.await.expect("request task shouldn't panic");
    assert_eq!(StatusCode::OK, req.status());
}

#[tokio::test]
async fn test_kill_aborts_request() {
    let app = byte_handler_app_builder()
        .spawn()
        .await
        .expect("failed building app");
    let addr = app.local_addr();
    let uri = format!("http://{}/", addr);
    let response = tokio::spawn(half_sec_post_request(uri));
    // leave some time for the request to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    app.shutdown()
        .await
        .expect("server should exit without error");

    let req = response
        .await
        .expect_err("request task should not complete");
    assert!(req.is_panic());
}

#[tokio::test]
async fn test_graceful_shutdown_abort_request_after_grace_period() {
    let (shutdown_tx, signal) = oneshot::channel::<()>();
    let shutdown =
        ShutdownSignal::custom(signal.map(|_| ())).grace_period(Duration::from_millis(100));
    let app = byte_handler_app_builder()
        .with_graceful_shutdown(shutdown)
        .spawn()
        .await
        .expect("failed building app");
    let addr = app.local_addr();
    let uri = format!("http://{}/", addr);
    let response = tokio::spawn(half_sec_post_request(uri));
    // leave some time for the request to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    let _ = shutdown_tx.send(());

    app.await.expect("server shutdown can't panic");
    let res = response
        .await
        .expect_err("request should panic with connection reset");
    assert!(res.is_panic());
}

#[tokio::test]
async fn test_graceful_shutdown_complete_request_within_grace_period() {
    let (shutdown_tx, signal) = oneshot::channel::<()>();
    let shutdown =
        ShutdownSignal::custom(signal.map(|_| ())).grace_period(Duration::from_millis(1000));
    let app = byte_handler_app_builder()
        .with_graceful_shutdown(shutdown)
        .spawn()
        .await
        .expect("failed building app");
    let addr = app.local_addr();
    let uri = format!("http://{}/", addr);
    let response = tokio::spawn(half_sec_post_request(uri));
    // leave some time for the request to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    let _ = shutdown_tx.send(());
    let res = response.await.expect("request should complete");

    app.await.expect("server should exit without errors");
    assert_eq!(res.status(), StatusCode::OK);
}
