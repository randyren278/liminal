//! Manual verification tool: connects to a running `liminald` and sends one real envelope.
//! Useful for testing the daemon's ingest path without needing camera/mic/Wi-Fi/BLE hardware or
//! permissions -- run `cargo run -p liminald` in one terminal, then
//! `cargo run -p liminald --example send_test_envelope` in another.

use std::io::Write;
use std::os::unix::net::UnixStream;

use liminal_ipc::EXPECTED_SCHEMA_VERSION;
use prost::Message;

fn main() {
    let uid = unsafe { libc::getuid() };
    let path = liminald::socket_path_for_uid(uid);

    let envelope = liminal_ipc::Envelope {
        schema_version: EXPECTED_SCHEMA_VERSION,
        message_id: format!("manual-test-{}", std::process::id()),
        sensor_stream_id: "camera".to_string(),
        monotonic_sequence: 1,
        captured_at_utc_us: 1_700_000_000_000_000,
        captured_at_mono_ns: 0,
        payload:
            br#"{"body_count":"one","joints":[{"name":"nose","x":0.5,"y":0.5,"confidence":0.9}]}"#
                .to_vec(),
    };

    let body = envelope.encode_to_vec();
    let mut frame = Vec::new();
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);

    println!("connecting to {}", path.display());
    let mut stream = UnixStream::connect(&path).expect("connect failed -- is liminald running?");
    stream.write_all(&frame).expect("write failed");
    println!("sent one envelope for stream 'camera'");
}
