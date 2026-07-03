//! Telnet listener and terminal adapter (Slice 8 / Slice 9).
//!
//! Boots a [`tokio::net::TcpListener`], allocates a node from the
//! application [`crate::app::node_pool::NodePool`] for every accepted
//! connection, performs telnet negotiation and delegates the BBS
//! workflow to [`crate::app::session_driver`]. All non-transport
//! wiring (driven adapters, configuration-derived policy values, node
//! pool sizing) lives in [`crate::app::runtime::Runtime`] — the
//! listener is handed an already-built runtime.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

use crate::app::colour_terminal::ColourTerminal;
use crate::app::node_pool::NodePool;
use crate::app::runtime::Runtime;
use crate::app::services::AppServices;
use crate::app::session_driver::SessionDriver;
use crate::app::terminal::{
    KeyRead, SessionSignal, Terminal, TerminalEcho, TerminalFuture, TerminalRead,
};
use crate::domain::session::LogonChannel;

use tokio::sync::mpsc;

use super::telnet_line::{read_telnet_key, read_telnet_line, EchoMode};

/// Bytes sent at the start of every accepted connection to set up
/// telnet line-mode in a way that is friendly to common clients:
///   - `IAC WILL SUPPRESS-GO-AHEAD` and `IAC DO SUPPRESS-GO-AHEAD`
///     enable full-duplex.
///   - `IAC WILL ECHO` lets the server echo input so the user can see
///     what they type even if their client doesn't echo locally.
const IAC_INIT: &[u8] = &[
    0xFF, 0xFB, 0x03, // IAC WILL SUPPRESS-GO-AHEAD
    0xFF, 0xFD, 0x03, // IAC DO   SUPPRESS-GO-AHEAD
    0xFF, 0xFB, 0x01, // IAC WILL ECHO
];

/// Sent to clients that arrive when every node is in use.
const BUSY_LINE: &[u8] = b"All BBS nodes are busy. Please try again later.\r\n";

/// Telnet listener bound to a socket and connected to an
/// already-wired [`Runtime`]. The listener itself owns no
/// configuration — its job ends at telnet negotiation.
pub struct TelnetListener {
    listener: TcpListener,
    runtime: Runtime,
}

impl TelnetListener {
    /// Binds a [`TcpListener`] on `addr` and stores the
    /// pre-constructed [`Runtime`] every accepted connection will
    /// share. Composition (driven adapters, policy values, node pool
    /// sizing) happens before this call — see
    /// [`crate::bootstrap::build_runtime`].
    ///
    /// # Errors
    /// Returns the underlying [`io::Error`] if the bind fails.
    pub async fn bind<A: ToSocketAddrs>(addr: A, runtime: Runtime) -> io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        Ok(Self { listener, runtime })
    }

    /// Returns the local address the listener is bound to.
    ///
    /// # Errors
    /// Returns the underlying [`io::Error`] if the address can't be
    /// queried.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Returns a clone of the shared [`NodePool`] handle.
    pub fn pool(&self) -> Arc<NodePool> {
        self.runtime.pool()
    }

    /// Accepts connections forever, spawning a per-session task for
    /// each one. Returns only on a listener error.
    ///
    /// # Errors
    /// Returns the underlying [`io::Error`] from `accept`.
    pub async fn run(&self) -> io::Result<()> {
        loop {
            let (stream, _peer) = self.listener.accept().await?;
            let runtime = self.runtime.clone();
            tokio::spawn(async move {
                // Boxed: the whole session state machine lives in this
                // future, whose stack footprint exceeds clippy's
                // `large_futures` threshold — heap-allocate it once per
                // connection instead of inflating every spawn.
                let _ = Box::pin(handle_connection(stream, runtime)).await;
            });
        }
    }
}

/// Per-connection task body.
///
/// Splits into "could allocate a node" and "couldn't"; on the happy
/// path the task lives until the client closes the connection. On the
/// busy path it writes the busy line and exits.
async fn handle_connection(mut stream: TcpStream, runtime: Runtime) -> io::Result<()> {
    let pool = runtime.pool();
    let Some(node_number) = pool.allocate().await else {
        stream.write_all(BUSY_LINE).await?;
        stream.flush().await?;
        return Ok(());
    };

    // The session's signal lane (July 2026 review, item 26): the
    // sender lives in the pool so other tasks can address this node;
    // the receiver rides inside the terminal, raced against the
    // socket. `release_node_after` clears the pool slot on the way
    // out, dropping the last sender.
    let (signal_tx, signal_rx) = mpsc::unbounded_channel();
    pool.attach_signal_sender(node_number, signal_tx).await;

    // Boxed for the same `large_futures` reason as the spawn site:
    // the driver future carries every sub-flow's locals inline.
    release_node_after(
        pool.clone(),
        node_number,
        Box::pin(async {
            stream.write_all(IAC_INIT).await?;
            // Wrap the transport terminal so the `M` command (Tier A
            // quickwin A8) can strip ANSI colour from output; a fresh
            // connection starts with colour on.
            let terminal =
                ColourTerminal::new(TelnetTerminal::new(&mut stream, Some(signal_rx)), true);
            let services: AppServices = runtime.services().clone();
            let mut driver =
                SessionDriver::new(terminal, node_number, LogonChannel::Remote, services);
            driver.run().await
        }),
    )
    .await
}

async fn release_node_after<F, T>(pool: Arc<NodePool>, node_number: u32, operation: F) -> T
where
    F: Future<Output = T>,
{
    let result = operation.await;
    let _ = pool.release(node_number).await;
    result
}

struct TelnetTerminal<'a> {
    stream: &'a mut TcpStream,
    pushback: Option<u8>,
    /// The in-progress line, hoisted out of the codec future (July
    /// 2026 review, item 26) so an interrupted `read_line` — raced
    /// against a session signal and dropped — resumes with the
    /// half-typed bytes (and their already-written echo) intact.
    line_buf: Vec<u8>,
    /// The session's signal lane: reads race the socket against this
    /// receiver so another task can deliver into the parked prompt.
    /// `None` once the channel closes (or for signal-less fixtures).
    signals: Option<mpsc::UnboundedReceiver<SessionSignal>>,
}

impl<'a> TelnetTerminal<'a> {
    fn new(
        stream: &'a mut TcpStream,
        signals: Option<mpsc::UnboundedReceiver<SessionSignal>>,
    ) -> Self {
        Self {
            stream,
            pushback: None,
            line_buf: Vec::new(),
            signals,
        }
    }
}

impl Terminal for TelnetTerminal<'_> {
    type Error = io::Error;

    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
        Box::pin(async move { self.stream.write_all(bytes).await })
    }

    fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
        Box::pin(async move { self.stream.flush().await })
    }

    fn read_line(
        &mut self,
        echo: TerminalEcho,
        timeout: Duration,
    ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
        Box::pin(async move {
            let deadline = tokio::time::sleep(timeout);
            tokio::pin!(deadline);
            loop {
                // Race the codec against the signal lane and the idle
                // deadline. Cancelling the codec future is safe: the
                // half-typed line and the CR-trailer pushback both live
                // on `self` (the item-26 hoist), so the next iteration
                // resumes exactly where the cancelled read stopped. A
                // signal landing mid-IAC-negotiation or mid-echo can
                // clip that exchange — negotiations happen at connect
                // and a lost echo byte is cosmetic, both accepted.
                let raced = {
                    let Self {
                        stream,
                        pushback,
                        line_buf,
                        signals,
                    } = &mut *self;
                    tokio::select! {
                        result = read_telnet_line(stream, pushback, line_buf, echo.into()) => {
                            Raced::Line(result)
                        }
                        signal = next_signal(signals) => Raced::Signal(signal),
                        () = &mut deadline => Raced::TimedOut,
                    }
                };
                match raced {
                    Raced::Line(result) => {
                        return match result? {
                            Some(line) => Ok(TerminalRead::Line(line)),
                            None => Ok(TerminalRead::Eof),
                        };
                    }
                    Raced::Key(_) => unreachable!("read_line races the line codec"),
                    Raced::Signal(Some(SessionSignal::Deliver(bytes))) => {
                        self.stream.write_all(&bytes).await?;
                        self.stream.flush().await?;
                    }
                    Raced::Signal(None) => {
                        // Channel closed (sender dropped): stop polling
                        // it — recv() on a closed channel returns
                        // immediately and would busy-loop the select.
                        self.signals = None;
                    }
                    Raced::TimedOut => {
                        // The idle timeout ends the session; drop the
                        // half-typed line so it cannot spill into a
                        // later read.
                        self.line_buf.clear();
                        return Ok(TerminalRead::IdleTimedOut);
                    }
                }
            }
        })
    }

    fn read_key(&mut self, timeout: Duration) -> TerminalFuture<'_, KeyRead, Self::Error> {
        Box::pin(async move {
            let deadline = tokio::time::sleep(timeout);
            tokio::pin!(deadline);
            loop {
                let raced = {
                    let Self {
                        stream,
                        pushback,
                        signals,
                        ..
                    } = &mut *self;
                    tokio::select! {
                        result = read_telnet_key(stream, pushback) => Raced::Key(result),
                        signal = next_signal(signals) => Raced::Signal(signal),
                        () = &mut deadline => Raced::TimedOut,
                    }
                };
                match raced {
                    Raced::Key(result) => {
                        return match result? {
                            Some(key) => Ok(KeyRead::Key(key)),
                            None => Ok(KeyRead::Eof),
                        };
                    }
                    Raced::Line(_) => unreachable!("read_key races the key codec"),
                    Raced::Signal(Some(SessionSignal::Deliver(bytes))) => {
                        self.stream.write_all(&bytes).await?;
                        self.stream.flush().await?;
                    }
                    Raced::Signal(None) => {
                        self.signals = None;
                    }
                    Raced::TimedOut => return Ok(KeyRead::IdleTimedOut),
                }
            }
        })
    }
}

/// Outcome of racing a codec read against the session-signal lane and
/// the idle deadline. Extracted from the `select!` arms so the arm
/// bodies run after the codec future (and its `&mut` borrows of the
/// terminal's fields) has been dropped.
enum Raced {
    Line(io::Result<Option<String>>),
    Key(io::Result<Option<crate::app::terminal::KeyEvent>>),
    Signal(Option<SessionSignal>),
    TimedOut,
}

/// Resolves the next session signal, or never if the session has no
/// signal lane (`None` — signal-less fixtures, or a closed channel).
async fn next_signal(
    signals: &mut Option<mpsc::UnboundedReceiver<SessionSignal>>,
) -> Option<SessionSignal> {
    match signals {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

impl From<TerminalEcho> for EchoMode {
    fn from(value: TerminalEcho) -> Self {
        match value {
            TerminalEcho::Visible => Self::Visible,
            TerminalEcho::Masked => Self::Masked,
        }
    }
}

#[cfg(test)]
mod tests;
