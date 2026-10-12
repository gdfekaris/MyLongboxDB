//! The socket filter, applied in a test process of its own, since it
//! cannot be lifted once applied.

use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::sync::Once;

fn blocked() {
    static APPLY: Once = Once::new();
    APPLY.call_once(|| mylongbox_sandbox::block_internet().unwrap());
}

fn refused<T>(result: std::io::Result<T>) -> bool {
    matches!(result, Err(e) if e.kind() == ErrorKind::PermissionDenied)
}

#[test]
fn internet_sockets_are_refused() {
    blocked();
    assert!(refused(TcpListener::bind("127.0.0.1:0")));
    assert!(refused(TcpListener::bind("[::1]:0")));
    assert!(refused(UdpSocket::bind("0.0.0.0:0")));
    assert!(refused(TcpStream::connect("192.0.2.1:80")));
    // Other families, such as netlink and raw packets, are refused too.
    for family in [libc::AF_NETLINK, libc::AF_PACKET, libc::AF_INET6] {
        // SAFETY: socket() takes plain integers and returns a new
        // descriptor or -1; nothing is read or written through memory.
        let fd = unsafe { libc::socket(family, libc::SOCK_DGRAM, 0) };
        assert_eq!(fd, -1, "family {family}");
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EACCES)
        );
    }
}

#[test]
fn local_sockets_still_work() {
    blocked();
    let (a, b) = UnixDatagram::pair().unwrap();
    a.send(b"hello").unwrap();
    let mut buf = [0; 5];
    b.recv(&mut buf).unwrap();
    assert_eq!(&buf, b"hello");
    let path = std::env::temp_dir().join(format!("mlbx-sandbox-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    UnixStream::connect(&path).unwrap();
    listener.accept().unwrap();
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn io_uring_cannot_be_set_up() {
    blocked();
    let mut params = [0u8; 120];
    // SAFETY: io_uring_setup reads and writes only the 120-byte
    // parameter block passed, which lives for the whole call.
    let fd = unsafe { libc::syscall(libc::SYS_io_uring_setup, 1, params.as_mut_ptr()) };
    assert_eq!(fd, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EACCES)
    );
}
