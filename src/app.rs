use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::Router;
use futures_util::future::FutureExt;
use futures_util::poll;
use hyper::Body;
use tokio::task::{JoinError, JoinHandle};

use crate::shutdown::ShutdownSignal;

/// Builder struct to configure [`App`] instances.
///
/// Constructed through [`App::builder`].
pub struct AppBuilder {
    addr: SocketAddr,
    router: Router<Body>,
    shutdown_listener: Option<ShutdownSignal>,
}

impl AppBuilder {
    pub(crate) fn new(addr: SocketAddr, router: Router<Body>) -> Self {
        Self {
            addr,
            router,
            shutdown_listener: None,
        }
    }

    /// Set a graceful shutdown listener for the server.
    ///
    /// Once the `shutdown_listener` future finishes, the server will receive a signal to
    /// gracefully shut down. For more information, refer to
    /// [`axum::Server::with_graceful_shutdown`].
    pub fn with_graceful_shutdown(mut self, shutdown: ShutdownSignal) -> Self {
        self.shutdown_listener = Some(shutdown);
        self
    }

    /// Spawn the [`App`] with the given configuration.
    pub fn spawn(self) -> App {
        let service_maker = self.router.into_make_service();
        let srv = axum::Server::bind(&self.addr).serve(service_maker);
        let actual_addr = srv.local_addr();
        tracing::debug!("app bound to {}", actual_addr);

        let handle = if let Some(listener) = self.shutdown_listener {
            tokio::spawn(
                srv.with_graceful_shutdown(listener.into_signal())
                    .map(|res| res.map_err(Into::into)),
            )
        } else {
            tracing::debug!("app running without graceful shutdown listener");
            tokio::spawn(srv.map(|res| res.map_err(Into::into)))
        };
        App {
            addr: actual_addr,
            handle,
        }
    }
}

/// Handle to a running App instance.
///
/// Can be configured and spawned through an [`AppBuilder`] obtained through [`App::builder`].
pub struct App {
    addr: SocketAddr,
    handle: JoinHandle<Result<(), ServerError>>,
}

impl App {
    /// Get an [`AppBuilder`] to build and spawn the App.
    ///
    /// # Example
    ///
    /// To run the [`App`] on port `8080`:
    ///
    /// ```
    /// # fn main() {}
    /// use axum::Router;
    /// use axum_convenience::App;
    ///
    /// async fn run(router: axum::Router) {
    ///     let app = App::builder(([127, 0, 0, 1], 8080).into(), router).spawn();
    /// }
    /// ```
    pub fn builder(addr: SocketAddr, router: Router) -> AppBuilder {
        AppBuilder::new(addr, router)
    }

    /// Returns the local address for the socket this server is listening on.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Kills the server by aborting the underlying tokio task.
    ///
    /// After aborting the task, this function awaits the task future. If the task future returns
    /// a panic error, we forward that error. If the returned error indicates cancellation, [`Ok`]
    /// is returned. If the server had already finished running before the `kill` call, that result
    /// is returned.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() {}
    /// use axum_convenience::App;
    ///
    /// async fn run_and_kill(app: App) {
    ///     app.kill().await.unwrap()
    /// }
    /// ```
    pub async fn kill(mut self) -> Result<(), ServerError> {
        tracing::debug!("aborting server task");
        if let Poll::Ready(res) = poll!(&mut self) {
            tracing::debug!("server task was already done");
            return res;
        }
        self.handle.abort();
        match self.handle.await {
            Err(e) => {
                if e.is_cancelled() {
                    return Ok(());
                }
                Err(ServerError::PanicError(e))
            }
            Ok(res) => res,
        }
    }
}

impl Future for App {
    type Output = Result<(), ServerError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Poll::Ready(rdy) = self.handle.poll_unpin(cx) {
            let res = match rdy {
                Ok(res) => res,
                Err(e) => {
                    if e.is_cancelled() {
                        Ok(())
                    } else if e.is_panic() {
                        Err(ServerError::PanicError(e))
                    } else {
                        // JoinError's inner `Repr` type is private so we can't get exhaustiveness checks here.
                        // Panicking might not be the nicest solution, but it points out if a new kind of JoinError
                        // should be introduced.
                        unreachable!("join errors should either be panic errors or cancelled")
                    }
                }
            };
            return Poll::Ready(res);
        }
        Poll::Pending
    }
}

/// Errors encountered while running an [`App`].
#[derive(thiserror::Error, Debug)]
pub enum ServerError {
    #[error("Server task panicked while running: {0}")]
    PanicError(JoinError),
    #[error("Server errored: {0}")]
    RuntimeError(#[from] hyper::Error),
}
