use axum::routing::get;
use axum::Router;
use hyper::Body;
use tracing_subscriber::EnvFilter;

use axum_convenience::{App, RustlsConfig, ShutdownSignal};

const KEY: &'static [u8] = include_bytes!("../self-signed-certs/localhost-key.pem");
const CERT: &'static [u8] = include_bytes!("../self-signed-certs/localhost.pem");

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
    let tls_config = RustlsConfig::from_pem(CERT.to_vec(), KEY.to_vec()).await?;

    let app = App::builder(([127, 0, 0, 1], 3000).into(), router)
        .with_graceful_shutdown(ShutdownSignal::os_signal())
        .with_tls(tls_config)
        .spawn()
        .await?;

    tracing::info!(
        "App listening at {}, waiting for shutdown signal",
        app.local_addr()
    );
    app.await?;
    Ok(())
}
