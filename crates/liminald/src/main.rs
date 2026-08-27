//! `liminald` binary -- see `lib.rs` for the module-level design notes.

use std::io;
use std::os::unix::net::UnixStream;
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

fn accept_connections<I>(incoming: I, connection_sender: &SyncSender<UnixStream>)
where
    I: IntoIterator<Item = io::Result<UnixStream>>,
{
    for stream in incoming {
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

fn spawn_connection_workers(
    receiver: &Arc<Mutex<Receiver<UnixStream>>>,
    ledger: &Arc<Mutex<SqliteLedger>>,
) {
    (0..CONNECTION_WORKERS).for_each(|_| {
        let receiver = Arc::clone(receiver);
        let ledger = Arc::clone(ledger);
        std::thread::spawn(move || run_connection_worker(receiver, ledger));
    });
}

#[cfg(not(test))]
fn main() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    let uid = unsafe { libc::getuid() };
    let socket_path = socket_path_for_uid(uid);

    prepare_socket_path(&socket_path).expect("failed to prepare socket path");
    let listener = UnixListener::bind(&socket_path).expect("failed to bind Unix socket");

    // §15: "0600 socket" -- the file only exists after bind(), so permissions are set here.
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
        .expect("failed to set socket file permissions");

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
    spawn_connection_workers(&connection_receiver, &ledger);
    accept_connections(listener.incoming(), &connection_sender);
}

#[cfg(test)]
mod tests {
    use super::*;
    use liminal_ipc::Envelope;
    use prost::Message;
    use std::io::Write;
    use std::net::Shutdown;
    use std::sync::mpsc::TrySendError;

    fn temp_ledger(name: &str) -> (std::path::PathBuf, Arc<Mutex<SqliteLedger>>) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "liminald-main-test-{name}-{}-{unique}.db",
            std::process::id()
        ));
        let ledger = SqliteLedger::open(&path, DEFAULT_MAX_SILENT_GAP_US).unwrap();
        (path, Arc::new(Mutex::new(ledger)))
    }

    fn sample_envelope(schema_version: u32) -> Envelope {
        Envelope {
            schema_version,
            message_id: format!("msg-{schema_version}"),
            sensor_stream_id: "camera".to_string(),
            monotonic_sequence: 1,
            captured_at_utc_us: 1_000,
            captured_at_mono_ns: 2_000,
            payload: br#"{"body_count":"one","joints":[]}"#.to_vec(),
        }
    }

    fn write_envelope(stream: &mut UnixStream, envelope: &Envelope) {
        let body = envelope.encode_to_vec();
        stream
            .write_all(&(body.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&body).unwrap();
    }

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

    #[test]
    fn handle_connection_ingests_a_valid_frame_then_clean_eof() {
        let (path, ledger) = temp_ledger("valid");
        let (server, mut client) = UnixStream::pair().unwrap();
        write_envelope(&mut client, &sample_envelope(1));
        client.shutdown(Shutdown::Write).unwrap();

        handle_connection(server, Arc::clone(&ledger));

        let event_count = ledger.lock().unwrap().events().unwrap().len();
        assert_eq!(event_count, 2);
        drop(ledger);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn handle_connection_surfaces_ingest_errors_without_killing_the_connection_loop() {
        let (path, ledger) = temp_ledger("ingest-error");
        let (server, mut client) = UnixStream::pair().unwrap();
        write_envelope(&mut client, &sample_envelope(999));
        client.shutdown(Shutdown::Write).unwrap();

        handle_connection(server, Arc::clone(&ledger));

        assert!(ledger.lock().unwrap().events().unwrap().is_empty());
        drop(ledger);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn handle_connection_closes_on_frame_read_error() {
        let (path, ledger) = temp_ledger("frame-error");
        let (server, mut client) = UnixStream::pair().unwrap();
        client
            .write_all(&((liminald::MAX_FRAME_BYTES + 1) as u32).to_be_bytes())
            .unwrap();
        client.shutdown(Shutdown::Write).unwrap();

        handle_connection(server, Arc::clone(&ledger));

        assert!(ledger.lock().unwrap().events().unwrap().is_empty());
        drop(ledger);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn connection_worker_processes_queued_stream_and_exits_when_sender_closes() {
        let (path, ledger) = temp_ledger("worker");
        let (sender, receiver) = connection_queue();
        let (server, client) = UnixStream::pair().unwrap();
        drop(client);
        sender.send(server).unwrap();
        drop(sender);

        run_connection_worker(receiver, Arc::clone(&ledger));

        assert!(ledger.lock().unwrap().events().unwrap().is_empty());
        drop(ledger);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn connection_worker_exits_immediately_when_queue_is_closed() {
        let (path, ledger) = temp_ledger("closed-worker");
        let (sender, receiver) = connection_queue();
        drop(sender);

        run_connection_worker(receiver, Arc::clone(&ledger));

        drop(ledger);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn accept_connections_queues_successes_and_tolerates_accept_errors() {
        let (sender, receiver) = sync_channel(2);
        let (server, _client) = UnixStream::pair().unwrap();
        let incoming = vec![
            Err(io::Error::new(io::ErrorKind::ConnectionAborted, "synthetic")),
            Ok(server),
        ];

        accept_connections(incoming, &sender);

        receiver.try_recv().unwrap();
    }

    #[test]
    fn accept_connections_stops_when_the_worker_queue_is_closed() {
        let (sender, receiver) = sync_channel(1);
        drop(receiver);
        let (server, _client) = UnixStream::pair().unwrap();

        accept_connections(vec![Ok(server)], &sender);
    }

    #[test]
    fn worker_count_is_nonzero() {
        assert!(CONNECTION_WORKERS > 0);
    }
}
