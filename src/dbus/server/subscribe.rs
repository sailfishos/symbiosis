// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Subscribing and sending interrupts.

use crate::{back_cover::InterruptReceiver, toh::traits::WaitInterrupt};
use futures::{future::pending, FutureExt};
use nix::sys::stat::fstat;
use std::io::{self, ErrorKind, Write};
use std::net::Shutdown;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use tokio::{
    io::{unix::AsyncFd, Interest},
    select, spawn,
    sync::{
        mpsc::{unbounded_channel, UnboundedSender},
        Mutex,
    },
    task::{JoinHandle, JoinSet},
};

/// Subscribers for interrupts.
pub(crate) struct Subscribers {
    subscribers: Arc<Mutex<Vec<UnixStream>>>,
    channel: UnboundedSender<Waited>,
    handle: Option<JoinHandle<()>>,
}

/// Subscription for interrupts.
pub(crate) struct Subscription(pub OwnedFd);

/// Allows to wait for peer socket to close.
struct Waited {
    socket: AsyncFd<UnixStream>,
    identity: (u64, u64),
}

impl Waited {
    /// Create new `Waited` for stream socket to wait for close.
    fn new(waited: UnixStream) -> io::Result<Self> {
        // fstat already here so we don't have to do it all the time.
        let identity = Self::identity(&waited);
        Ok(Self {
            socket: AsyncFd::new(waited)?,
            identity,
        })
    }

    /// Helper to fstat the identity of the given stream socket.
    fn identity(fd: &UnixStream) -> (u64, u64) {
        let stat = fstat(fd.as_raw_fd()).expect("fstat succeeds on valid socket file descriptors");
        (stat.st_dev, stat.st_ino)
    }

    /// Checks if the stream socket is the same as the one we are waiting for.
    fn matches_stream(&self, stream: &UnixStream) -> bool {
        Self::identity(stream) == self.identity
    }

    /// Waits for `Waited` peer to close and returns it.
    async fn wait_until_closed(waited: Self) -> Self {
        loop {
            match waited.socket.ready(Interest::WRITABLE).await {
                Err(error) => {
                    log::error!(
                        "Unexpected error while waiting for subscriber socket to close: {error}"
                    );
                    break;
                }
                Ok(mut guard) => {
                    if guard.ready().is_write_closed() {
                        break;
                    }
                    guard.clear_ready();
                }
            }
        }
        waited
    }
}

impl Subscribers {
    /// Create new instance.
    pub fn new(mut receiver: InterruptReceiver) -> Self {
        let subscribers: Arc<Mutex<Vec<UnixStream>>> = Default::default();
        let (channel, mut streams_to_wait) = unbounded_channel::<Waited>();
        let handle = Some({
            let arc = subscribers.clone();
            spawn(async move {
                enum Branch {
                    Interrupt,
                    ClosedSocket(Waited),
                }

                let mut waiting_tasks: Option<JoinSet<Waited>> = None;
                loop {
                    let closed = if let Some(join_set) = &mut waiting_tasks {
                        join_set.join_next().boxed()
                    } else {
                        pending().boxed()
                    };

                    let branch = select! {
                        result = receiver.wait_interrupt() => {
                            if result.is_ok() {
                                Branch::Interrupt
                            } else {
                                // All interrupts have been received.
                                break;
                            }
                        }
                        waited = streams_to_wait.recv() => {
                            if let Some(waited) = waited {
                                log::debug!("Add task for subscription socket");
                                // TODO: Rust 1.83.0 would allow to use get_or_insert_default.
                                waiting_tasks.get_or_insert_with(Default::default).spawn(Waited::wait_until_closed(waited));
                            } else {
                                // This should not happen.
                                log::error!("Channel was closed, no new sockets are registered");
                            }
                            continue;
                        }
                        result = closed => {
                            match result {
                                Some(Ok(waited)) => Branch::ClosedSocket(waited),
                                Some(Err(error)) => {
                                    log::error!("Failure to wait for subscriber sockets: {error}");
                                    continue;
                                }
                                None => {
                                    // All tasks were used, clean up.
                                    waiting_tasks.take();
                                    continue;
                                }
                            }
                        }
                    };

                    let mut guard = arc.lock().await;
                    match branch {
                        Branch::Interrupt => {
                            // Serve subscribers until sending end is closed.
                            log::debug!("New interrupt to send");
                            guard.retain(Self::send_token);
                        }
                        Branch::ClosedSocket(waited) => {
                            // A socket closed, remove the matching one!
                            log::debug!("Unsubscribed from interrupt notifications");
                            guard.retain(|subscriber| !waited.matches_stream(subscriber));
                        }
                    }
                }
                log::debug!("Stopped sending interrupt notifications");
            })
        });

        Self {
            subscribers,
            channel,
            handle,
        }
    }

    /// Abort and join the background task.
    pub async fn abort(&mut self) {
        if let Some(handle) = self.handle.take() {
            if !handle.is_finished() {
                handle.abort();
            }
            let _ = handle.await;
        }
    }

    /// Create file descriptor that represents a subscription.
    pub async fn create_subscription(&mut self) -> io::Result<Subscription> {
        let (send, recv) = UnixStream::pair()?;
        send.set_nonblocking(true)?;
        let waited = Waited::new(send.try_clone()?)?;
        self.subscribers.lock().await.push(send);
        if self.channel.send(waited).is_err() {
            log::warn!("Receiving end had been closed, cannot wait for socket close");
        }
        recv.shutdown(Shutdown::Write)?;
        Ok(Subscription(recv.as_fd().try_clone_to_owned()?))
    }

    /// Sends token to given socket and returns whether the socket is still good to use.
    fn send_token(mut socket: &UnixStream) -> bool {
        const TOKEN: &[u8; 1] = b"\0";
        match socket.write(TOKEN) {
            Ok(_) => true,
            Err(error) if error.kind() == ErrorKind::WouldBlock => true,
            Err(error) if error.kind() == ErrorKind::BrokenPipe => {
                // Subscriber closed the receiving end, it can be cleaned up.
                false
            }
            Err(error) => {
                log::error!(
                    "Unexpected error while sending interrupt notification to subscriber: {error}"
                );
                false
            }
        }
    }
}
