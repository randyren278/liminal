//! `liminald` binary -- see `lib.rs` for the module-level design notes.

use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};

use liminal_ledger::SqliteLedger;
use liminald::{
    ingest_envelope, prepare_socket_path, read_length_delimited_envelope, socket_path_for_uid,
};

/// §17 Storage Locations: canonical data lives under `~/Library/Application Support/Liminal/`.
fn db_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let dir = std::path::PathBuf::from(home).join("Library/Application Support/Liminal");
    std::fs::create_dir_all(&dir).expect("failed to create Application Support directory");
    dir.join("liminal.db")
}

fn handle_connection(stream: UnixStream, ledger: Arc<Mutex<SqliteLedger>>) {
    let mut stream = stream;
    loop {
        match read_length_delimited_envelope(&mut stream) {
            Ok(Some(envelope)) => {
                let stream_id = envelope.sensor_stream_id.clone();
                let mut ledger = ledger.lock().expect("ledger mutex poisoned");
                match ingest_envelope(&mut ledger, &envelope) {
                    Ok(()) => println!("liminald: ingested observation from stream '{stream_id}'"),
                    Err(e) => {
                        eprintln!("liminald: failed to ingest envelope from '{stream_id}': {e}")
                    }
                }
            }
            Ok(None) => {
                println!("liminald: client disconnected");
                break;
            }
            Err(e) => {
                eprintln!("liminald: frame read error, closing connection: {e}");
                break;
            }
        }
    }
}

fn main() {
    let uid = unsafe { libc::getuid() };
    let socket_path = socket_path_for_uid(uid);

    prepare_socket_path(&socket_path).expect("failed to prepare socket path");
    let listener = UnixListener::bind(&socket_path).expect("failed to bind Unix socket");

    // §15: "0600 socket" -- the file only exists after bind(), so permissions are set here.
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
            .expect("failed to set socket file permissions");
    }

    println!("liminald: listening on {}", socket_path.display());

    let db = db_path();
    println!("liminald: opening ledger at {}", db.display());
    // §93 performance budget doesn't specify this constant; 30s is a placeholder generous enough
    // not to spuriously flag gaps during normal camera-frame-rate jitter, tightened once a real
    // per-organ cadence is established.
    let ledger = Arc::new(Mutex::new(
        SqliteLedger::open(&db, 30_000_000).expect("failed to open ledger"),
    ));

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                println!("liminald: accepted a connection");
                let ledger = Arc::clone(&ledger);
                std::thread::spawn(move || handle_connection(stream, ledger));
            }
            Err(e) => eprintln!("liminald: accept error: {e}"),
        }
    }
}
