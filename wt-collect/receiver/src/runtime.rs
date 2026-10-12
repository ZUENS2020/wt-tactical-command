//! One owner task per room. Network handlers only enqueue bounded, coalesced observations.
use crate::live::Hub;
use serde_json::Value;
use std::{collections::{HashMap, HashSet}, path::PathBuf, sync::{Arc, Mutex}};
use tokio::sync::{broadcast, mpsc, oneshot};
use wtc_common::{now_ms, Record};

struct Feed { session: String, player: String, device: String, nick: String, record: Record, parsed: Option<Value> }
enum Command { Draw(Value, String, oneshot::Sender<Result<String, String>>), Round(oneshot::Sender<()>), Flush(oneshot::Sender<()>) }
pub struct RoomHandle {
    pending: Mutex<HashMap<(String, String), Feed>>,
    commands: mpsc::Sender<Command>,
    pub tx: broadcast::Sender<Arc<str>>,
    hello: Mutex<String>,
}
impl RoomHandle {
    pub fn hello(&self) -> String { self.hello.lock().unwrap().clone() }
    pub fn offer(&self, session: &str, player: &str, device: &str, nick: &str, r: Record) -> bool {
        let parsed = r.body.as_deref().and_then(|b| serde_json::from_str::<Value>(b).ok());
        let mut q = self.pending.lock().unwrap();
        let key = (session.into(), r.ep.clone());
        if q.len() >= 2048 && !q.contains_key(&key) { return false; }
        if q.get(&key).map(|f| (f.record.mono_ns, f.record.seq) >= (r.mono_ns, r.seq)).unwrap_or(false) { return false; }
        let bytes: usize = q.values().map(|f| f.record.body.as_ref().map(String::len).unwrap_or(0)).sum();
        if bytes + r.body.as_ref().map(String::len).unwrap_or(0) > 8 * 1024 * 1024 { return false; }
        q.insert(key, Feed { session: session.into(), player: player.into(), device: device.into(), nick: nick.into(), record: r, parsed }); true
    }
    pub async fn draw(&self, op: Value, client: String) -> Result<String, String> {
        let (tx, rx) = oneshot::channel();
        self.commands.send(Command::Draw(op, client, tx)).await.map_err(|_| "room_closed")?;
        rx.await.map_err(|_| "room_closed")?
    }
    pub async fn new_round(&self) {
        let (tx, rx) = oneshot::channel();
        if self.commands.send(Command::Round(tx)).await.is_ok() { let _ = rx.await; }
    }
}
pub struct Runtime {
    root: PathBuf,
    pub default_room: String,
    pub view_key: String,
    room_map: HashMap<String, String>,
    rooms: Mutex<HashMap<String, Arc<RoomHandle>>>,
    images: Arc<Mutex<HashSet<String>>>,
}
impl Runtime {
    pub fn new(root: &std::path::Path) -> Self {
        let hub = Hub::from_env(root);
        Self { root: root.into(), default_room: hub.default_room, view_key: hub.view_key, room_map: hub.room_map,
            rooms: Mutex::new(HashMap::new()), images: Arc::new(Mutex::new(HashSet::new())) }
    }
    pub fn image_ready(&self, hash: String) { self.images.lock().unwrap().insert(hash); }
    pub fn for_player(&self, player: &str) -> Arc<RoomHandle> {
        self.room(self.room_map.get(player).map(String::as_str).unwrap_or(&self.default_room))
    }
    pub fn room(&self, name: &str) -> Arc<RoomHandle> {
        let mut rooms = self.rooms.lock().unwrap();
        if let Some(room) = rooms.get(name) { return room.clone(); }
        let mut hub = Hub::from_env(&self.root);
        hub.default_room = name.into();
        let tx = hub.room(name).tx.clone();
        let (commands, mut rx) = mpsc::channel(128);
        let h = Arc::new(RoomHandle { pending: Mutex::new(HashMap::new()), commands, tx, hello: Mutex::new(hub.room(name).hello(now_ms())) });
        let room_key = name.to_string();
        let (worker, images, name) = (h.clone(), self.images.clone(), name.to_string());
        // Serial disk worker: flush acknowledgements are sent only after the final save.
        type Save = (Vec<(PathBuf, Vec<u8>)>, Option<oneshot::Sender<()>>);
        let (save_tx, mut save_rx) = mpsc::channel::<Save>(2);
        let (failed_tx, mut failed_rx) = mpsc::channel::<()>(2);
        tokio::spawn(async move {
            while let Some((files, ack)) = save_rx.recv().await {
                let result = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                    for (p, bytes) in files {
                        let tmp = p.with_extension("json.tmp");
                        std::fs::write(&tmp, bytes)?; std::fs::rename(tmp, p)?;
                    } Ok(())
                }).await;
                if !matches!(result, Ok(Ok(()))) { eprintln!("room persistence failed: {result:?}"); let _ = failed_tx.try_send(()); }
                if let Some(ack) = ack { let _ = ack.send(()); }
            }
        });
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = tick.tick() => {
                        let mut inputs: Vec<_> = worker.pending.lock().unwrap().drain().map(|(_, f)| f).collect();
                        inputs.sort_by_key(|f| (f.record.ep != "/map_info.json", f.record.ts_server_ms.unwrap_or(f.record.wall_ms), f.record.seq));
                        hub.ready_images = images.lock().unwrap().clone();
                        for f in inputs { hub.feed_source_parsed(&f.session, &f.player, &f.device, &f.nick, &name, &f.record, now_ms(), f.parsed.as_ref()); }
                        hub.tick(now_ms());
                    }
                    cmd = rx.recv() => match cmd {
                        Some(Command::Draw(op, client, ack)) => { let _ = ack.send(hub.draw_op(&name, &op, &client, now_ms())); }
                        Some(Command::Round(ack)) => { hub.new_round(&name, now_ms()); let _ = ack.send(()); }
                        Some(Command::Flush(ack)) => {
                            hub.mark_dirty();
                            let files = hub.take_persistence();
                            let (done_tx, done_rx) = oneshot::channel();
                            if save_tx.send((files, Some(done_tx))).await.is_ok() { let _ = done_rx.await; }
                            let _ = ack.send(());
                        }
                        None => break,
                    }
                }
                *worker.hello.lock().unwrap() = hub.room(&name).hello(now_ms());
                if failed_rx.try_recv().is_ok() { hub.mark_dirty(); }
                let files = hub.take_persistence();
                if !files.is_empty() && save_tx.try_send((files, None)).is_err() { hub.mark_dirty(); }
            }
        });
        rooms.insert(room_key, h.clone()); h
    }
    pub async fn flush(&self) {
        let rooms: Vec<_> = self.rooms.lock().unwrap().values().cloned().collect();
        for room in rooms {
            let (tx, rx) = oneshot::channel();
            if room.commands.send(Command::Flush(tx)).await.is_ok() { let _ = rx.await; }
        }
    }
}
