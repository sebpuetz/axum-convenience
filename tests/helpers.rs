#![allow(dead_code)]
use std::convert::Infallible;
use std::time::Duration;

use axum::body::Bytes;
use axum::response::Response;
use axum::routing::{any, get};
use axum::Router;
use axum_convenience::{App, AppBuilder};
use hyper::{Body, Client, StatusCode};
use once_cell::sync::Lazy;

pub const HELLO_WORLD: &'static str = "hello world";

pub static _TRACING_INIT: Lazy<()> = Lazy::new(|| {
    let env_filter = tracing_subscriber::EnvFilter::try_from_env("TRACE_TESTS")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt::SubscriberBuilder::default()
        .with_env_filter(env_filter)
        .init();
});

pub fn hello_world_app_builder() -> AppBuilder {
    *_TRACING_INIT;
    let router = Router::new().route(
        "/hello-world",
        get(move || async move {
            let span = tracing::Span::current();
            tracing::info!(parent: span, "serving hello world");
            HELLO_WORLD
        }),
    );
    let addr = ([127, 0, 0, 1], 0).into();
    App::builder(addr, router)
}

pub fn byte_handler_app_builder() -> AppBuilder {
    *_TRACING_INIT;
    let router = Router::new().route(
        "/",
        any(move |_bytes: Bytes| async {
            let span = tracing::Span::current();
            tracing::info!(parent: span, "bytes response");
            StatusCode::OK
        }),
    );
    let addr = ([127, 0, 0, 1], 0).into();
    App::builder(addr, router)
}

pub async fn send_empty_get(uri: String) -> Response<Body> {
    let req = hyper::Request::get(uri)
        .body(Body::empty())
        .expect("failed to build request");
    let client = Client::new();
    client.request(req).await.expect("request failed")
}

pub async fn half_sec_post_request(uri: String) -> Response<Body> {
    const CHUNK: &'static [u8] = &[0; 1024];
    let data = async_stream::stream! {
        for _ in 0..5u8 {
            yield Ok::<_, Infallible>(CHUNK);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };

    let req = hyper::Request::post(uri)
        .body(Body::wrap_stream(data))
        .expect("failed to build request");
    let client = Client::new();
    client.request(req).await.expect("request failed")
}
