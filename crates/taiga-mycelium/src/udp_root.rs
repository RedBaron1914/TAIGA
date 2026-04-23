use crate::{Needle, Root, TreeId, TreeInfo, RouteUpdate};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

/// Внутренний протокол для симуляции сети по UDP
#[derive(Serialize, Deserialize, Debug)]
pub enum UdpPacket {
    DiscoverRequest(TreeInfo, Vec<RouteUpdate>),
    DiscoverResponse(TreeInfo, Vec<RouteUpdate>),
    NeedlePayload(Needle),
}

#[derive(Clone)]
pub struct UdpRoot {
    socket: Arc<UdpSocket>,
    local_info: Arc<Mutex<TreeInfo>>,
    local_port: u16,
    peers: Arc<Mutex<HashMap<TreeId, (SocketAddr, TreeInfo, Vec<RouteUpdate>)>>>,
    needle_rx: Arc<Mutex<mpsc::Receiver<(TreeId, Needle)>>>,
    cached_id: String,
}

impl UdpRoot {
    pub async fn new(port: u16, local_info: TreeInfo) -> Result<Self, String> {
        let addr = format!("0.0.0.0:{}", port);
        let socket = UdpSocket::bind(&addr).await.map_err(|e| e.to_string())?;
        
        log::info!("[UdpRoot] Привязан к локальному порту: {}", port);

        let socket = Arc::new(socket);
        let peers = Arc::new(Mutex::new(HashMap::new()));
        let (needle_tx, needle_rx) = mpsc::channel(100);
        let cached_id = format!("udp-root-{}", local_info.id);
        let local_info = Arc::new(Mutex::new(local_info));

        let sock_clone = socket.clone();
        let peers_clone = peers.clone();
        let local_info_clone = local_info.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 65535];
            loop {
                if let Ok((len, addr)) = sock_clone.recv_from(&mut buf).await {
                    if let Ok(packet) = serde_json::from_slice::<UdpPacket>(&buf[..len]) {
                        match packet {
                            UdpPacket::DiscoverRequest(info, routes) => {
                                let current_info = local_info_clone.lock().await.clone();
                                if info.id != current_info.id {
                                    log::info!("[UdpRoot] Получен запрос на поиск от Дерева: {} (IP: {})", info.id, addr);
                                    peers_clone.lock().await.insert(info.id, (addr, info, routes));
                                    
                                    let resp = UdpPacket::DiscoverResponse(current_info, vec![]); // We'll rely on active discover() to send our routes
                                    if let Ok(bytes) = serde_json::to_vec(&resp) {
                                        let _ = sock_clone.send_to(&bytes, addr).await;
                                    }
                                }
                            }
                            UdpPacket::DiscoverResponse(info, routes) => {
                                log::info!("[UdpRoot] Найден сосед! Дерево: {} (IP: {})", info.id, addr);
                                peers_clone.lock().await.insert(info.id, (addr, info, routes));
                            }
                            UdpPacket::NeedlePayload(needle) => {
                                log::info!("[UdpRoot] Получена Хвоинка от {}", addr);
                                let mut sender_id = uuid::Uuid::nil();
                                for (id, (p_addr, _, _)) in peers_clone.lock().await.iter() {
                                    if *p_addr == addr {
                                        sender_id = *id;
                                        break;
                                    }
                                }
                                let _ = needle_tx.send((sender_id, needle)).await;
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            socket,
            local_info,
            local_port: port,
            peers,
            needle_rx: Arc::new(Mutex::new(needle_rx)),
            cached_id,
        })
    }
}

#[async_trait]
impl Root for UdpRoot {
    fn id(&self) -> String {
        format!("udp-root-{}", self.local_port)
    }

    async fn discover(&self, local_routes: Vec<RouteUpdate>) -> Result<Vec<(TreeInfo, Vec<RouteUpdate>)>, String> {
        log::info!("[UdpRoot] Пускаем корни... Ищем соседей!");
        let info = self.local_info.lock().await.clone();
        let req = UdpPacket::DiscoverRequest(info, local_routes);
        let bytes = serde_json::to_vec(&req).map_err(|e| e.to_string())?;

        for p in 40000..=40020 {
            if p != self.local_port {
                let addr = format!("127.0.0.1:{}", p);
                let _ = self.socket.send_to(&bytes, &addr).await;
            }
        }
        
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let peers = self.peers.lock().await;
        Ok(peers.values().map(|(_, info, routes)| (info.clone(), routes.clone())).collect())
    }

    async fn update_local_info(&self, info: TreeInfo) {
        *self.local_info.lock().await = info;
    }

    async fn send_needle(&self, to: TreeId, needle: Needle) -> Result<(), String> {
        let packet = UdpPacket::NeedlePayload(needle);
        let bytes = serde_json::to_vec(&packet).map_err(|e| e.to_string())?;

        let peers = self.peers.lock().await;
        
        if to == Uuid::nil() {
            // Режим "Шёпот Леса": рассылаем пакет всем соседям, которых мы знаем
            for (addr, _, _) in peers.values() {
                let _ = self.socket.send_to(&bytes, addr).await;
            }
            Ok(())
        } else {
            // Обычная отправка конкретному узлу
            if let Some((addr, _, _)) = peers.get(&to) {
                self.socket.send_to(&bytes, addr).await.map_err(|e| e.to_string())?;
                Ok(())
            } else {
                Err(format!("Tree {} not found in local roots", to))
            }
        }
    }

    async fn receive_needle(&self) -> Result<(TreeId, Needle), String> {
        let mut rx = self.needle_rx.lock().await;
        if let Some(res) = rx.recv().await {
            Ok(res)
        } else {
            Err("UdpRoot listener closed".to_string())
        }
    }

    fn is_connected(&self) -> bool {
        true
    }
}
