//! The socket filter reaches threads that were running before it was
//! applied. A test process of its own, so the filter is surely applied
//! after the thread starts.

use std::io::ErrorKind;
use std::net::UdpSocket;

#[test]
fn threads_started_earlier_are_covered_too() {
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let earlier = std::thread::spawn(move || {
        wait.recv().unwrap();
        UdpSocket::bind("0.0.0.0:0")
    });
    mylongbox_sandbox::block_internet().unwrap();
    go.send(()).unwrap();
    let result = earlier.join().unwrap();
    assert!(
        matches!(&result, Err(e) if e.kind() == ErrorKind::PermissionDenied),
        "{result:?}"
    );
}
