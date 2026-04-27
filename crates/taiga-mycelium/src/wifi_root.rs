use crate::{Needle, Root, TreeId, TreeInfo, RouteUpdate};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TcpPacket {
    DiscoverRequest(TreeInfo, Vec<RouteUpdate>),
    DiscoverResponse(TreeInfo, Vec<RouteUpdate>),
    NeedlePayload(Needle),
}

/// Узел Wi-Fi Direct (работает по TCP для максимальной скорости)
#[derive(Clone)]
pub struct WifiRoot {
    local_info: Arc<Mutex<TreeInfo>>,
    peers: Arc<Mutex<HashMap<TreeId, (mpsc::Sender<TcpPacket>, TreeInfo, Vec<RouteUpdate>)>>>,
    needle_rx: Arc<Mutex<mpsc::Receiver<(TreeId, Needle)>>>,
    cached_id: String,
    /// Канал для трансляции всем подключенным пирам (для discover и broadcast)
    broadcast_tx: mpsc::Sender<TcpPacket>,
}

impl WifiRoot {
    /// Если мы Group Owner (GO), поднимаем сервер.
    /// Если мы клиент, подключаемся к GO.
    pub async fn new(local_info: TreeInfo, group_owner_ip: String, is_group_owner: bool) -> Result<Self, String> {
        let (needle_tx, needle_rx) = mpsc::channel(1000);
        let peers = Arc::new(Mutex::new(HashMap::new()));
        let local_info = Arc::new(Mutex::new(local_info.clone()));
        let cached_id = format!("wifi-root-{}", local_info.lock().await.id);
        
        let (broadcast_tx, mut broadcast_rx) = mpsc::channel::<TcpPacket>(100);

        let root = Self {
            local_info: local_info.clone(),
            peers: peers.clone(),
            needle_rx: Arc::new(Mutex::new(needle_rx)),
            cached_id,
            broadcast_tx,
        };

        // Запускаем рассыльщик броадкастов (Шепот Леса / Discover) по всем TCP соединениям
        let peers_for_bcast = peers.clone();
        tokio::spawn(async move {
            while let Some(packet) = broadcast_rx.recv().await {
                let peers_lock = peers_for_bcast.lock().await;
                for (tx, _, _) in peers_lock.values() {
                    let _ = tx.send(packet.clone()).await;
                }
            }
        });

        if is_group_owner {
            // Поднимаем сервер
            let listener = TcpListener::bind("0.0.0.0:40001").await.map_err(|e| e.to_string())?;
            log::info!("[WifiRoot] Мы Group Owner. Сервер запущен на порту 40001");
            #[cfg(target_os = "android")]
            crate::jni_bridge::send_ui_log("WIFI", "Мы Group Owner. Ожидаем P2P-клиентов на порту 40001.");

            let local_info_clone = local_info.clone();
            let peers_clone = peers.clone();
            let needle_tx_clone = needle_tx.clone();
            tokio::spawn(async move {
                while let Ok((stream, addr)) = listener.accept().await {
                    log::info!("[WifiRoot] Новое входящее TCP-подключение от {}", addr);
                    #[cfg(target_os = "android")]
                    crate::jni_bridge::send_ui_log("WIFI", &format!("Новое входящее P2P-подключение от {}", addr));
                    Self::handle_connection(stream, local_info_clone.clone(), peers_clone.clone(), needle_tx_clone.clone());
                }
            });
        } else {
            // Подключаемся к GO
            let go_addr = format!("{}:40001", group_owner_ip);
            log::info!("[WifiRoot] Подключаемся к Group Owner: {}", go_addr);
            #[cfg(target_os = "android")]
            crate::jni_bridge::send_ui_log("WIFI", &format!("Подключаемся к Group Owner: {}", go_addr));

            // В реальной жизни нужно делать ретраи, так как сервер мог еще не подняться
            let stream = TcpStream::connect(&go_addr).await.map_err(|e| e.to_string())?;
            log::info!("[WifiRoot] Успешно подключено к GO!");
            #[cfg(target_os = "android")]
            crate::jni_bridge::send_ui_log("WIFI", "Успешно подключено к Group Owner!");
            Self::handle_connection(stream, local_info.clone(), peers.clone(), needle_tx.clone());
        }

        Ok(root)
    }

    fn handle_connection(
        stream: TcpStream, 
        local_info: Arc<Mutex<TreeInfo>>, 
        peers: Arc<Mutex<HashMap<TreeId, (mpsc::Sender<TcpPacket>, TreeInfo, Vec<RouteUpdate>)>>>,
        needle_tx: mpsc::Sender<(TreeId, Needle)>
    ) {
        tokio::spawn(async move {
            let (mut reader, mut writer) = stream.into_split();
            let (tx, mut rx) = mpsc::channel::<TcpPacket>(100);
            
            // Писатель в TCP
            tokio::spawn(async move {
                while let Some(packet) = rx.recv().await {
                    if let Ok(bytes) = serde_json::to_vec(&packet) {
                        let len = (bytes.len() as u32).to_be_bytes();
                        if writer.write_all(&len).await.is_err() { break; }
                        if writer.write_all(&bytes).await.is_err() { break; }
                    }
                }
            });

            // Сразу после подключения шлем DiscoverRequest
            let info = local_info.lock().await.clone();
            let _ = tx.send(TcpPacket::DiscoverRequest(info, vec![])).await; // Для старта пусто
            
            let mut length_buf = [0u8; 4];
            loop {
                if reader.read_exact(&mut length_buf).await.is_err() { break; }
                let len = u32::from_be_bytes(length_buf) as usize;
                
                let mut buf = vec![0u8; len];
                if reader.read_exact(&mut buf).await.is_err() { break; }
                
                if let Ok(packet) = serde_json::from_slice::<TcpPacket>(&buf) {
                    match packet {
                        TcpPacket::DiscoverRequest(peer_info, routes) => {
                            log::info!("[WifiRoot] Получен запрос на поиск от {}", peer_info.id);
                            peers.lock().await.insert(peer_info.id, (tx.clone(), peer_info, routes));
                            
                            let info = local_info.lock().await.clone();
                            let _ = tx.send(TcpPacket::DiscoverResponse(info, vec![])).await;
                        }
                        TcpPacket::DiscoverResponse(peer_info, routes) => {
                            log::info!("[WifiRoot] Ответ от соседа: {}", peer_info.id);
                            peers.lock().await.insert(peer_info.id, (tx.clone(), peer_info, routes));
                        }
                        TcpPacket::NeedlePayload(needle) => {
                            // Ищем кто отправил по наличию tx в мапе (немного неоптимально, но для прототипа сойдет)
                            let mut sender_id = Uuid::nil();
                            for (id, (p_tx, _, _)) in peers.lock().await.iter() {
                                if p_tx.same_channel(&tx) {
                                    sender_id = *id;
                                    break;
                                }
                            }
                            let _ = needle_tx.send((sender_id, needle)).await;
                        }
                    }
                }
            }
            log::info!("[WifiRoot] TCP-соединение закрыто.");
        });
    }
}

#[async_trait]
impl Root for WifiRoot {
    fn id(&self) -> String {
        self.cached_id.clone()
    }

    async fn discover(&self, local_routes: Vec<RouteUpdate>) -> Result<Vec<(TreeInfo, Vec<RouteUpdate>)>, String> {
        let info = self.local_info.lock().await.clone();
        let req = TcpPacket::DiscoverRequest(info, local_routes);
        let _ = self.broadcast_tx.send(req).await;
        
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        
        let peers = self.peers.lock().await;
        Ok(peers.values().map(|(_, info, routes)| (info.clone(), routes.clone())).collect())
    }

    async fn update_local_info(&self, info: TreeInfo) {
        *self.local_info.lock().await = info;
    }

    async fn send_needle(&self, to: TreeId, needle: Needle) -> Result<(), String> {
        let packet = TcpPacket::NeedlePayload(needle);

        if to == Uuid::nil() {
            let _ = self.broadcast_tx.send(packet).await;
            Ok(())
        } else {
            let peers = self.peers.lock().await;
            if let Some((tx, _, _)) = peers.get(&to) {
                let _ = tx.send(packet).await;
                Ok(())
            } else {
                Err(format!("Узел {} не найден в Wi-Fi сети", to))
            }
        }
    }

    async fn receive_needle(&self) -> Result<(TreeId, Needle), String> {
        let mut rx = self.needle_rx.lock().await;
        if let Some(res) = rx.recv().await {
            Ok(res)
        } else {
            Err("WifiRoot listener closed".to_string())
        }
    }

    fn is_connected(&self) -> bool {
        true
    }
}
