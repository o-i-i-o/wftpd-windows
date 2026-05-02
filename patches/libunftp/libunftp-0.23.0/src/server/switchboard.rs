use crate::server::session::{Session, SharedSession};
use crate::server::shutdown::Notifier;
use dashmap::{DashMap, Entry};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::ops::RangeInclusive;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::Mutex;
use unftp_core::auth::UserDetail;
use unftp_core::storage::StorageBackend;

/// Identifies a passive listening port entry in the Switchboard that is associated with a specific
/// session. The key is the passive listening port that has been reserved for the client.
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub(crate) struct SwitchboardKey {
    port: u16,
}

impl SwitchboardKey {
    fn new(port: u16) -> Self {
        SwitchboardKey { port }
    }
}

impl From<&SocketAddrPair> for SwitchboardKey {
    fn from(connection: &SocketAddrPair) -> Self {
        SwitchboardKey::new(connection.destination.port())
    }
}

type SessionHandle<S, U> = Weak<Mutex<Session<S, U>>>;

const PORT_COOLDOWN_SECS: u64 = 2;

/// Connect clients to the right data channel
#[derive(Debug)]
pub(in crate::server) struct Switchboard<S, U>
where
    S: StorageBackend<U>,
    U: UserDetail,
{
    switchboard: Arc<DashMap<SwitchboardKey, SessionHandle<S, U>>>,
    port_range: RangeInclusive<u16>,
    logger: slog::Logger,
    cooldown_ports: Arc<Mutex<HashSet<u16>>>,
}

#[derive(Debug)]
pub(in crate::server) enum SwitchboardError {
    // SwitchBoardNotInitialized,
    EntryNotAvailable,
    // EntryCreationFailed,
    MaxRetriesError,
}

impl<S, U> Switchboard<S, U>
where
    S: StorageBackend<U> + 'static,
    U: UserDetail + 'static,
{
    pub fn new(logger: slog::Logger, passive_ports: RangeInclusive<u16>) -> Self {
        let board = Arc::new(DashMap::new());
        Self {
            switchboard: board,
            port_range: passive_ports,
            logger,
            cooldown_ports: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn start_scavenger(&self, shutdown_topic: Arc<Notifier>) {
        let board = self.switchboard.clone();
        let logger = self.logger.clone();
        tokio::spawn(async move {
            let mut shutdown_listener = shutdown_topic.subscribe().await;
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {
                        slog::debug!(logger, "Scavenging switchboard");
                        board.retain(|_, session| {
                            let session_exists = session.upgrade().is_some();
                            if !session_exists {
                                slog::info!(logger, "Scavenging zombie switchboard entry (session gone)");
                            }
                            session_exists
                        });
                    }
                    _ = shutdown_listener.listen() => {
                        slog::info!(logger, "Switchboard scavenger shutting down.");
                        break;
                    }
                }
            }
        });
    }

    pub async fn try_and_claim(&mut self, key: SwitchboardKey, session_arc: SharedSession<S, U>) -> Result<(), SwitchboardError> {
        // Atomically insert the key and value into the switchboard hashmap
        match self.switchboard.entry(key) {
            Entry::Occupied(_) => Err(SwitchboardError::EntryNotAvailable),
            Entry::Vacant(entry) => {
                entry.insert(Arc::downgrade(&session_arc));
                Ok(())
            }
        }
    }

    pub fn unregister_by_connection_pair(&mut self, connection: &SocketAddrPair) {
        let hash = connection.into();

        self.unregister_by_key(&hash)
    }

    pub fn unregister_by_key(&mut self, key: &SwitchboardKey) {
        if self.switchboard.remove(key).is_none() {
            slog::warn!(self.logger, "Entry already removed? key: {:?}", key);
        }

        let cooldown_ports = self.cooldown_ports.clone();
        let port = key.port;
        let logger = self.logger.clone();
        tokio::spawn(async move {
            {
                let mut cooldown = cooldown_ports.lock().await;
                cooldown.insert(port);
            }
            tokio::time::sleep(Duration::from_secs(PORT_COOLDOWN_SECS)).await;
            {
                let mut cooldown = cooldown_ports.lock().await;
                cooldown.remove(&port);
            }
            slog::debug!(logger, "Port {} cooldown expired, available for reuse", port);
        });
    }

    #[tracing_attributes::instrument]
    pub async fn get_session_by_connection_pair(&mut self, connection: &SocketAddrPair) -> Option<SharedSession<S, U>> {
        let key: SwitchboardKey = connection.into();

        self.switchboard.get(&key).and_then(|entry| entry.value().upgrade())
    }

    /// Find the next available port within the specified range (inclusive of the upper limit).
    /// The reserved port is associated with the session, using a hashmap keyed by port only.
    pub async fn reserve(&mut self, session_arc: SharedSession<S, U>) -> Result<u16, SwitchboardError> {
        let range_size = self.port_range.end() - self.port_range.start();

        let randomized_initial_port = {
            let mut data = [0; 2];
            getrandom::fill(&mut data).expect("Error generating random free port to reserve");
            u16::from_ne_bytes(data)
        };

        let cooldown_snapshot = {
            let cooldown = self.cooldown_ports.lock().await;
            cooldown.clone()
        };

        for i in 0..=range_size {
            let port = self.port_range.start() + ((randomized_initial_port + i) % range_size);
            slog::debug!(self.logger, "Trying if port {} is available", port);

            if cooldown_snapshot.contains(&port) {
                slog::debug!(self.logger, "Port {} is in cooldown, skipping", port);
                continue;
            }

            let key = SwitchboardKey::new(port);

            match &self.try_and_claim(key.clone(), session_arc.clone()).await {
                Ok(_) => {
                    let mut session = session_arc.lock().await;
                    if let Some(active_datachan_key) = &session.switchboard_active_datachan
                        && active_datachan_key != &key
                    {
                        slog::info!(self.logger, "Removing stale session data channel {:?}", &active_datachan_key);
                        self.unregister_by_key(active_datachan_key);
                    }

                    session.switchboard_active_datachan = Some(key);
                    return Ok(port);
                }
                Err(_) => {
                    slog::debug!(self.logger, "Port entry is occupied (key: {:?}), trying to find a vacant one", &key);
                    continue;
                }
            }
        }

        slog::warn!(self.logger, "Out of tries reserving next free port!");
        Err(SwitchboardError::MaxRetriesError)
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct SocketAddrPair {
    pub source: SocketAddr,
    pub destination: SocketAddr,
}
