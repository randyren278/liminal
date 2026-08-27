//! `liminald` binary -- see `lib.rs` for the module-level design notes.

use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};

use liminal_ledger::{default_db_path, SqliteLedger, DEFAULT_MAX_SILENT_GAP_US};
use liminald::{
    ingest_envelope, prepare_socket_path, read_length_delimited_envelope, socket_path_for_uid,
};

const MAX_PENDING_CONNECTIONS: usize = 16;
const CONNECTION_WORKERS: usize = 4;

fn connection_queue() -> (SyncSender<UnixStream>, Arc<Mutex<Receiver<UnixStream>>>) {
    let (sender, receiver) = sync_channel(MAX_PENDING_CONNECTIONS);
    (sender, Arc::new(Mutex::new(receiver)))
}

fn run_connection_worker(
    receiver: Arc<Mutex<Receiver<UnixStream>>>,
    ledger: Arc<Mutex<SqliteLedger>>,
) {
    loop {
        let stream = match receiver
            .lock()
            .expect("connection queue mutex poisoned")
            .recv()
        {
            Ok(stream) => stream,
            Err(_) => break,
        };
        handle_connection(stream, Arc::clone(&ledger));
    }
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

    let db = default_db_path();
    std::fs::create_dir_all(db.parent().unwrap())
        .expect("failed to create Application Support directory");
    println!("liminald: opening ledger at {}", db.display());
    // §93 performance budget doesn't specify this constant; 30s is a placeholder generous enough
    // not to spuriously flag gaps during normal camera-frame-rate jitter, tightened once a real
    // per-organ cadence is established.
    let ledger = Arc::new(Mutex::new(
        SqliteLedger::open(&db, DEFAULT_MAX_SILENT_GAP_US).expect("failed to open ledger"),
    ));

    let (connection_sender, connection_receiver) = connection_queue();
    for _ in 0..CONNECTION_WORKERS {
        let receiver = Arc::clone(&connection_receiver);
        let ledger = Arc::clone(&ledger);
        std::thread::spawn(move || run_connection_worker(receiver, ledger));
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                println!("liminald: accepted a connection");
                // A synchronous bounded queue provides backpressure at the connection boundary:
                // once all workers and 16 queued clients are occupied, accept-loop progress
                // pauses instead of creating unbounded threads or memory pressure.
                if let Err(error) = connection_sender.send(stream) {
                    eprintln!("liminald: connection queue closed: {error}");
                    break;
                }
            }
            Err(e) => eprintln!("liminald: accept error: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::TrySendError;

    #[test]
    fn connection_queue_applies_a_hard_pending_limit() {
        let (sender, _receiver) = connection_queue();
        let mut streams = Vec::new();
        for _ in 0..MAX_PENDING_CONNECTIONS {
            let (stream, _peer) = UnixStream::pair().unwrap();
            streams.push(stream);
        }
        for stream in streams {
            sender.try_send(stream).unwrap();
        }
        let (overflow, _peer) = UnixStream::pair().unwrap();
        assert!(matches!(
            sender.try_send(overflow),
            Err(TrySendError::Full(_))
        ));
    }
}
