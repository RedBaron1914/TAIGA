use crate::{Needle, Root, TreeId, TreeInfo, RouteUpdate};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;
use std::collections::HashMap;

/// Транспорт для Bluetooth LE. 
/// На Android работает через JNI и системные сервисы Kotlin.
/// На Desktop пока остается заглушкой.
#[derive(Clone)]
pub struct BleRoot {
    local_info: Arc<Mutex<TreeInfo>>,
    cached_id: String,
    /// Входящие Хвоинки, полученные через JNI
    needle_tx: mpsc::Sender<(TreeId, Needle)>,
    needle_rx: Arc<Mutex<mpsc::Receiver<(TreeId, Needle)>>>,
    /// Карта соответствия ID узла и его (MAC-адрес, timestamp_secs, pubkey_hash)
    mac_map: Arc<Mutex<HashMap<TreeId, (String, u64, [u8; 4])>>>,
    /// Список недавно обнаруженных соседей для метода discover
    discovered_neighbors: Arc<Mutex<Vec<(TreeInfo, Vec<RouteUpdate>)>>>,
}

impl BleRoot {
    pub fn new(local_info: TreeInfo) -> Self {
        let (tx, rx) = mpsc::channel(1000);
        let cached_id = format!("ble-root-{}", local_info.id);
        
        Self {
            local_info: Arc::new(Mutex::new(local_info)),
            cached_id,
            needle_tx: tx,
            needle_rx: Arc::new(Mutex::new(rx)),
            mac_map: Arc::new(Mutex::new(HashMap::new())),
            discovered_neighbors: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Вызывается из JNI, когда сканер нашел новое устройство. 
    /// Ожидаем, что в Manufacturer Data зашиты: [ID (16 байт), Freedom (1 байт), Status (1 байт), PubKeyHash (4 байта)]
    /// Возвращает список старых TreeId, которые были вытеснены (MAC сменил Node ID при совпадении крипто-хэша).
    pub async fn add_discovered_neighbor(&self, mac: String, manufacturer_data: Vec<u8>) -> Vec<TreeId> {
        if manufacturer_data.len() < 16 {
            return vec![]; // Слишком короткий пакет
        }

        let mut id_bytes = [0u8; 16];
        id_bytes.copy_from_slice(&manufacturer_data[0..16]);
        let id = match Uuid::from_slice(&id_bytes) {
            Ok(uuid) => uuid,
            Err(_) => return vec![],
        };

        let pubkey_hash = if manufacturer_data.len() >= 22 {
            let mut h = [0u8; 4];
            h.copy_from_slice(&manufacturer_data[18..22]);
            h
        } else {
            [0u8; 4]
        };

        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

        let mut map = self.mac_map.lock().await;
        let mac_lower = mac.to_ascii_lowercase();

        // Очистка устаревших записей в mac_map (старше 300 секунд / 5 минут) для предотвращения утечки памяти (OOM)
        map.retain(|_, (_, timestamp, _)| now.saturating_sub(*timestamp) < 300);

        // Защита от MAC Spoofing: Вытесняем старый ID только если крипто-хэш совпадает (или если равен 0).
        // Если кто-то вещает с тем же MAC, но с другим крипто-хэшем — это попытка спуфинга!
        let evicted: Vec<TreeId> = map.iter()
            .filter(|(old_id, (old_mac, _, old_hash))| {
                old_mac.to_ascii_lowercase() == mac_lower && **old_id != id &&
                (pubkey_hash == [0u8; 4] || *old_hash == [0u8; 4] || pubkey_hash == *old_hash)
            })
            .map(|(old_id, _)| *old_id)
            .collect();

        map.retain(|old_id, (old_mac, _, old_hash)| {
            !(old_mac.to_ascii_lowercase() == mac_lower && *old_id != id &&
              (pubkey_hash == [0u8; 4] || *old_hash == [0u8; 4] || pubkey_hash == *old_hash))
        });
        map.insert(id, (mac, now, pubkey_hash));
        
        let freedom = if manufacturer_data.len() >= 17 {
            match manufacturer_data[16] {
                0 => crate::FreedomLevel::None,
                1 => crate::FreedomLevel::WhitelistOnly,
                2 => crate::FreedomLevel::Normal,
                3 => crate::FreedomLevel::Full,
                _ => crate::FreedomLevel::None,
            }
        } else {
            crate::FreedomLevel::None
        };

        let is_virtual_uplink = if manufacturer_data.len() >= 18 {
            manufacturer_data[17] == 1
        } else {
            false
        };

        // В реальном протоколе здесь должен быть P2P-хендшейк для обмена маршрутами.
        // Пока просто добавляем узел как соседа с пустыми маршрутами.
        let mut neighbors = self.discovered_neighbors.lock().await;
        // Удаляем из очереди обнаружения все ранее вытесненные ID для этого MAC
        neighbors.retain(|(info, _)| !evicted.contains(&info.id));
        neighbors.push((
            TreeInfo {
                id,
                status: crate::NodeStatus::Tree,
                public_key: vec![], // Будет получено позже
                freedom,
                is_virtual_uplink,
            },
            vec![]
        ));
        
        evicted
    }

    /// Вызывается из JNI, когда получено сообщение по Bluetooth GATT
    pub async fn inject_needle(&self, sender_mac: String, needle_bytes: Vec<u8>) {
        if let Ok(needle) = serde_json::from_slice::<Needle>(&needle_bytes) {
            // Пытаемся найти ID по MAC
            let mut sender_id = Uuid::nil();
            let sender_mac_lower = sender_mac.to_ascii_lowercase();
            let map = self.mac_map.lock().await;
            for (id, (mac, _, _)) in map.iter() {
                if mac.to_ascii_lowercase() == sender_mac_lower {
                    sender_id = *id;
                    break;
                }
            }
            let _ = self.needle_tx.send((sender_id, needle)).await;
        }
    }
}

#[async_trait]
impl Root for BleRoot {
    fn id(&self) -> String {
        self.cached_id.clone()
    }

    async fn update_local_info(&self, info: TreeInfo) {
        *self.local_info.lock().await = info;
    }

    async fn update_local_routes(&self, _routes: Vec<RouteUpdate>) {
        // BLE currently does not support pushing routes over advertising data due to size limits.
        // This will be implemented in v1.1.0 as part of the full GATT handshake.
    }

    async fn discover(&self, _local_routes: Vec<RouteUpdate>) -> Result<Vec<(TreeInfo, Vec<RouteUpdate>)>, String> {
        // Возвращаем накопленных за время сканирования соседей и очищаем список
        let mut neighbors = self.discovered_neighbors.lock().await;
        let result = neighbors.clone();
        neighbors.clear();
        Ok(result)
    }

    async fn send_needle(&self, _to: TreeId, _needle: Needle) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            let mac_map = self.mac_map.lock().await;
            if let Some((mac, _, _)) = mac_map.get(&_to) {
                if let Ok(payload) = serde_json::to_vec(&_needle) {
                    log::info!("[BleRoot] Отправка Хвоинки на MAC: {}", mac);
                    // Вызов через мост JNI
                    crate::jni_bridge::send_ble_message_to_kotlin(mac, &payload);
                    return Ok(());
                }
            }
            return Err("MAC-адрес узла не найден".to_string());
        }

        #[cfg(not(target_os = "android"))]
        {
            let _ = _to;
            let _ = _needle;
            Err("BLE отправка не поддерживается на этой платформе".to_string())
        }
    }

    async fn receive_needle(&self) -> Result<(TreeId, Needle), String> {
        let mut rx = self.needle_rx.lock().await;
        if let Some(res) = rx.recv().await {
            Ok(res)
        } else {
            Err("BleRoot channel closed".to_string())
        }
    }

    fn is_connected(&self) -> bool {
        true
    }
}
