//! Unix domain socket への connect（タイムアウト付き）。`std::UnixStream::connect` は待ち切れない。

use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use socket2::{Domain, SockAddr, Socket, Type};

/// `path` へ stream 接続する。`timeout` 以内に完了しなければ `TimedOut`。
pub fn connect_unix_stream(path: &Path, timeout: Duration) -> io::Result<UnixStream> {
    let addr = SockAddr::unix(path)?;
    let socket = Socket::new(Domain::UNIX, Type::STREAM, None)?;
    socket.connect_timeout(&addr, timeout)?;
    let owned: OwnedFd = socket.into();
    Ok(UnixStream::from(owned))
}
