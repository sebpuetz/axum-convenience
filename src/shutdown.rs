use futures_util::future::BoxFuture;
use futures_util::FutureExt;

/// Different kinds of shutdown signals for an [`crate::app::App`].
pub enum ShutdownSignal {
    /// Shutdown is signalled by SIGTERM or Ctrl-c
    OsSignal,
    /// Custom shutdown handler triggered by the future finishing.
    Custom(BoxFuture<'static, ()>),
}

impl ShutdownSignal {
    pub(crate) fn into_signal(self) -> BoxFuture<'static, ()> {
        match self {
            ShutdownSignal::OsSignal => Self::listen_for_signals(),
            ShutdownSignal::Custom(custom_signal) => custom_signal,
        }
    }

    fn listen_for_signals() -> BoxFuture<'static, ()> {
        #[cfg(unix)]
        let signal = async {
            use tokio::signal::unix::*;
            let mut sigterm =
                signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
            tokio::select! {
                _ = sigterm.recv() => {
                    tracing::info!("received SIGTERM")
                },
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("received Ctrl-C")
                }
            }
        };
        #[cfg(not(unix))]
        let signal = async {
            let _ = tokio::signal::ctrl_c().await;
        }
        .boxed();
        signal.boxed()
    }
}
