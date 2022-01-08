use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Router;
use axum_server::service::SendService;
use axum_server::Handle;
use futures_util::future::FutureExt;
use hyper::server::conn::AddrStream;
use hyper::Body;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::task::{JoinError, JoinHandle};
use tracing::{Instrument, Span};

use crate::shutdown::ShutdownSignal;

/// Builder struct to configure [`App`] instances.
///
/// Constructed through [`App::builder`].
#[derive(Debug)]
pub struct AppBuilder {
    addr: SocketAddr,
    router: Router<Body>,
    shutdown_listener: Option<ShutdownSignal>,
    #[cfg(feature = "tls")]
    tls: Option<axum_server::tls_rustls::RustlsConfig>,
}

impl AppBuilder {
    pub(crate) fn new(addr: SocketAddr, router: Router<Body>) -> Self {
        Self {
            addr,
            router,
            shutdown_listener: None,
            #[cfg(feature = "tls")]
            tls: None,
        }
    }

    /// Set a graceful shutdown listener for the server.
    ///
    /// Once the `signal` future finishes, the server will receive a signal to gracefully shut
    /// down. If a grace period has been configured via [`ShutdownSignal::grace_period`],
    /// connections will be aborted if they don't complete within that period.
    pub fn with_graceful_shutdown(mut self, signal: ShutdownSignal) -> Self {
        self.shutdown_listener = Some(signal);
        self
    }

    /// Configure TLS for the server.
    #[cfg(feature = "tls")]
    pub fn with_tls(mut self, tls: axum_server::tls_rustls::RustlsConfig) -> Self {
        self.tls = Some(tls);
        self
    }

    /// Spawn the [`App`] with the given configuration.
    ///
    /// The [`App`] is immediately served after this call.
    pub async fn spawn(self) -> Result<App, ServerError> {
        #[cfg(feature = "tls")]
        {
            let mut slf = self;
            if let Some(tls_config) = slf.tls.take() {
                let acceptor = axum_server::tls_rustls::RustlsAcceptor::new(tls_config);
                return slf.spawn_with_acceptor(acceptor).await;
            } else {
                let acceptor = axum_server::accept::DefaultAcceptor::default();
                return slf.spawn_with_acceptor(acceptor).await;
            }
        }
        #[cfg(not(feature = "tls"))]
        {
            let acceptor = axum_server::accept::DefaultAcceptor::default();
            self.spawn_with_acceptor(acceptor).await
        }
    }

    async fn spawn_with_acceptor<A>(self, acceptor: A) -> Result<App, ServerError>
    where
        A: axum_server::accept::Accept<AddrStream, Router> + Clone + Send + Sync + 'static,
        A::Stream: AsyncRead + AsyncWrite + Unpin + Send,
        A::Service: SendService<hyper::Request<hyper::Body>> + Send,
        A::Future: Send,
    {
        let server_handle = Handle::new();
        let service_maker = self.router.into_make_service();
        // spawn the server task here so we can safely retrieve the bound address
        let server_span = tracing::debug_span!("server", address = tracing::field::Empty);
        let mut server = tokio::spawn(
            axum_server::Server::bind(self.addr)
                .acceptor(acceptor)
                .handle(server_handle.clone())
                .serve(service_maker)
                .map(|res| -> Result<(), ServerError> {
                    tracing::trace!("inner server task finished");
                    Ok(res?)
                })
                .instrument(server_span.clone()),
        );

        let addr = tokio::select! {
            addr = server_handle.listening() => {
                server_span.record("address", &tracing::field::display(addr));
                addr
            },
            server_dead = &mut server => {
                server_dead.map_err(ServerError::PanicError)??;
                return Err(ServerError::BuildError)
            }
        };

        let task_handle = if let Some(listener) = self.shutdown_listener {
            let graceful_shutdown_handle = server_handle.clone();
            let server_span = server_span.clone();
            let srv_fut = async move {
                tokio::select! {
                    grace_period = listener.wait_for_signal() => {
                        tracing::debug!("received signal for graceful shutdown");
                        graceful_shutdown_handle.graceful_shutdown(grace_period);
                        server.await.map_err(ServerError::PanicError)??;
                    },
                    srv_res = &mut server => {
                        tracing::debug!("server stopped running without graceful shutdown");
                        srv_res.map_err(ServerError::PanicError)??;
                    }
                };
                Ok(())
            };
            tokio::spawn(srv_fut.instrument(server_span))
        } else {
            tracing::debug!(
                parent: &server_span,
                "app running without graceful shutdown listener"
            );
            server
        };
        tracing::debug!(parent: &server_span, "app started");
        Ok(App {
            addr,
            task_handle,
            server_handle,
            server_span,
        })
    }
}

/// Handle to a running App instance.
///
/// Can be configured and spawned through an [`AppBuilder`]. Builders can be obtained through
/// [`App::builder`].
pub struct App {
    addr: SocketAddr,
    server_handle: axum_server::Handle,
    task_handle: JoinHandle<Result<(), ServerError>>,
    server_span: Span,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App").field("addr", &self.addr).finish()
    }
}

impl App {
    /// Get an [`AppBuilder`] to build and spawn the App.
    ///
    /// The underlying server starts listening immediately after calling [`AppBuilder::spawn`].
    ///
    /// # Example
    ///
    /// To run the [`App`] on port `8080`:
    ///
    /// ```
    /// # fn main() {}
    /// use axum::Router;
    /// use axum_convenience::{App, ServerError};
    ///
    /// async fn run(router: Router) -> Result<(), ServerError> {
    ///     let addr = ([127, 0, 0, 1], 8080).into();
    ///     App::builder(addr, router)
    ///         .spawn()
    ///         .await?
    ///         .await
    /// }
    /// ```
    pub fn builder(addr: SocketAddr, router: Router) -> AppBuilder {
        AppBuilder::new(addr, router)
    }

    /// Return the local address for the socket this server is listening on.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Shutdown the server without waiting for open connections.
    ///
    /// After signalling shutdown, this method returns [`Shutdown`] which is a future returning
    /// once the underlying tokio task has returned.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() {}
    /// use axum_convenience::App;
    ///
    /// async fn run_and_shutdown(app: App) {
    ///     app.shutdown().await.unwrap()
    /// }
    /// ```
    pub fn shutdown(self) -> Shutdown {
        tracing::debug!(parent: &self.server_span, "shutting down server task");
        self.server_handle.shutdown();
        Shutdown {
            span: self.server_span,
            handle: self.task_handle,
        }
    }

    /// Explicitly request graceful shutdown.
    ///
    /// This method returns a [`Shutdown`] struct which is a future returning the result of the
    /// underlying tokio task.
    #[must_use = "shutdown will be requested immediately, awaiting `Shutdown` means waiting for the actual shutdown"]
    pub fn graceful_shutdown(self, grace_period: Option<Duration>) -> Shutdown {
        tracing::debug!(parent: &self.server_span, "gracefully shutting down server task");
        self.server_handle.graceful_shutdown(grace_period);
        Shutdown {
            span: self.server_span,
            handle: self.task_handle,
        }
    }
}

impl Future for App {
    type Output = Result<(), ServerError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let span = self.server_span.clone();
        let _guard = span.enter();
        let handle = Pin::new(&mut self.task_handle);
        poll_app_res(handle, cx)
    }
}

pub struct Shutdown {
    span: Span,
    handle: JoinHandle<Result<(), ServerError>>,
}

impl Future for Shutdown {
    type Output = Result<(), ServerError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let span = self.span.clone();
        let _guard = span.enter();
        let handle = Pin::new(&mut self.handle);
        poll_app_res(handle, cx)
    }
}

#[tracing::instrument(skip(slf, cx))]
fn poll_app_res(
    mut slf: Pin<&mut JoinHandle<Result<(), ServerError>>>,
    cx: &mut Context<'_>,
) -> Poll<Result<(), ServerError>> {
    if let Poll::Ready(rdy) = slf.poll_unpin(cx) {
        tracing::trace!("app has stopped running");
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

/// Errors encountered while running an [`App`].
#[derive(thiserror::Error, Debug)]
pub enum ServerError {
    #[error("Server task panicked while running: {0}")]
    PanicError(JoinError),
    #[error("Server errored: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Unexpected error while building the server")]
    BuildError,
}

#[cfg(test)]
mod test {
    use std::net::SocketAddr;

    use axum::Router;

    use crate::{App, ServerError};

    #[tokio::test]
    async fn test_conflicting_spawns() {
        // if `App::spawn` calls `Handle::listening().await` before actually trying to bind to the
        // socket the future gets stuck waiting for the server's address notification. This test
        // guards against such errors by ensuring we're getting returning an error instead of
        // getting stuck.
        let addr = SocketAddr::from(([127, 0, 0, 1], 0));
        let router = Router::new();
        let app = App::builder(addr, router.clone())
            .spawn()
            .await
            .expect("Should be able to bind first app");
        let addr = app.local_addr();

        let err = App::builder(addr, router)
            .spawn()
            .await
            .expect_err("Should not be able to bind to the same port as first app");
        match err {
            ServerError::IoError(e) => {
                assert_eq!(std::io::ErrorKind::AddrInUse, e.kind());
            }
            e => {
                panic!("expected ServerError::BuildError, not: {:?}", e)
            }
        }
    }
}
