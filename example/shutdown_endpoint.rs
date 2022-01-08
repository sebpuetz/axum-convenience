use axum::extract::Extension;
use axum::routing::get;
use axum::{AddExtensionLayer, Router};
use futures_util::FutureExt;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use axum_convenience::{App, ShutdownSignal};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("axum_convenience=debug,shutdown_endpoint=debug"));
    tracing_subscriber::fmt::SubscriberBuilder::default()
        .with_env_filter(env_filter)
        .init();
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(5);
    let (kill_tx, mut kill_rx) = mpsc::channel::<()>(5);
    let router = Router::new()
        .route(
            "/stop",
            get(shutdown).layer(AddExtensionLayer::new(ShutdownTx(shutdown_tx))),
        )
        .route(
            "/stopstopstop",
            get(kill).layer(AddExtensionLayer::new(KillTx(kill_tx))),
        );

    let mut app = App::builder(([127, 0, 0, 1], 3000).into(), router)
        .with_graceful_shutdown(ShutdownSignal::custom(
            async move {
                let _ = shutdown_rx.recv().await;
            }
            .boxed(),
        ))
        .spawn()
        .await?;
    let addr = app.local_addr();

    tracing::info!(
        "App listening at {}, graceful shutdown through GET http://{}/stop kill by GET /stopstopstop",
        addr,
        addr
    );
    tokio::select! {
        app_res = &mut app => {
            app_res?;
        },
        _ = kill_rx.recv() => {
            app.shutdown().await?;
        }
    };
    Ok(())
}

#[derive(Clone)]
struct KillTx(mpsc::Sender<()>);

async fn kill(tx: Extension<KillTx>) {
    let Extension(KillTx(tx)) = tx;
    let _ = tx.send(()).await;
}

#[derive(Clone)]
struct ShutdownTx(mpsc::Sender<()>);

async fn shutdown(tx: Extension<ShutdownTx>) {
    tracing::info!("shutting down!");
    let Extension(ShutdownTx(tx)) = tx;
    let _ = tx.send(()).await;
}
