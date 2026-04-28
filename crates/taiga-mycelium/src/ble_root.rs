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
    /// Карта соответствия ID узла и его физического MAC-адреса
    mac_map: Arc<Mutex<HashMap<TreeId, String>>>,
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
    /// Ожидаем, что в Manufacturer Data зашиты: [ID (16 байт), Freedom (1 байт), Status (1 байт)]
    pub async fn add_discovered_neighbor(&self, mac: String, manufacturer_data: Vec<u8>) {
        if manufacturer_data.len() < 16 {
            return; // Слишком короткий пакет
        }

        let mut id_bytes = [0u8; 16];
        id_bytes.copy_from_slice(&manufacturer_data[0..16]);
        let id = match Uuid::from_slice(&id_bytes) {
            Ok(uuid) => uuid,
            Err(_) => return,
        };

        let mut map = self.mac_map.lock().await;
        map.insert(id, mac);
        
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
    }

    /// Вызывается из JNI, когда получено сообщение по Bluetooth GATT
    pub async fn inject_needle(&self, sender_mac: String, needle_bytes: Vec<u8>) {
        if let Ok(needle) = serde_json::from_slice::<Needle>(&needle_bytes) {
            // Пытаемся найти ID по MAC
            let mut sender_id = Uuid::nil();
            let map = self.mac_map.lock().await;
            for (id, mac) in map.iter() {
                if mac == &sender_mac {
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
            if let Some(mac) = mac_map.get(&_to) {
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
