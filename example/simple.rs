use axum::routing::get;
use axum::Router;
use hyper::Body;
use tracing_subscriber::EnvFilter;

use axum_convenience::{App, ShutdownSignal};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt::SubscriberBuilder::default()
        .with_env_filter(env_filter)
        .init();
    let router = Router::new().route(
        "/hello-world",
        get::<_, _, Body>(|| async { "hello world" }),
    );

    let app = App::builder(([127, 0, 0, 1], 3000).into(), router)
        .with_graceful_shutdown(ShutdownSignal::OsSignal)
        .spawn();

    tracing::info!(
        "App listening at {}, waiting for shutdown signal",
        app.local_addr()
    );
    app.await?;
    Ok(())
}
