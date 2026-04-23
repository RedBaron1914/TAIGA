use crate::{Needle, Root, TreeId, TreeInfo, NodeStatus};
use async_trait::async_trait;
use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use std::str::FromStr;
use lazy_static::lazy_static;

// Определяем уникальные UUID для нашей "Тайги", по которым мы будем отличать наши узлы
// от умных часов, наушников и холодильников вокруг.
lazy_static! {
    /// Service UUID: "Это устройство поддерживает протокол Тайги"
    static ref TAIGA_SERVICE_UUID: uuid::Uuid = uuid::Uuid::from_str("7A16A000-0000-4000-8000-000000000000").unwrap();
    
    /// Characteristic UUID: Канал для отправки "Хвоинок" (Needle) на это устройство
    static ref TAIGA_RX_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_str("7A16A001-0000-4000-8000-000000000000").unwrap();
}

pub struct BleRoot {
    adapter: Adapter,
    local_info: Arc<Mutex<TreeInfo>>,
    cached_id: String,
}

impl BleRoot {
    pub async fn new(local_info: TreeInfo) -> Result<Self, String> {
        let manager = Manager::new().await.map_err(|e| e.to_string())?;
        
        let adapters = manager.adapters().await.map_err(|e| e.to_string())?;
        let adapter = adapters.into_iter().next().ok_or("Нет доступных Bluetooth-адаптеров")?;
        
        log::info!("[BleRoot] Bluetooth адаптер найден и подключен!");

        let cached_id = format!("ble-root-{}", local_info.id);

        Ok(Self {
            adapter,
            local_info: Arc::new(Mutex::new(local_info)),
            cached_id,
        })
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

    async fn discover(&self, _local_routes: Vec<crate::RouteUpdate>) -> Result<Vec<(TreeInfo, Vec<crate::RouteUpdate>)>, String> {
        log::info!("[BleRoot] Начинаем BLE-сканирование Корней...");
        
        // ВАЖНО: Фильтруем эфир, чтобы видеть ТОЛЬКО устройства с сервисом Тайги
        let filter = ScanFilter {
            services: vec![*TAIGA_SERVICE_UUID]
        };

        self.adapter
            .start_scan(filter)
            .await
            .map_err(|e| e.to_string())?;

        // Слушаем эфир 2 секунды
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let peripherals = self.adapter.peripherals().await.map_err(|e| e.to_string())?;
        let mut neighbors = Vec::new();
        
        for peripheral in peripherals {
            if let Ok(Some(props)) = peripheral.properties().await {
                if props.services.contains(&*TAIGA_SERVICE_UUID) {
                    log::info!("[BleRoot] Найдено Дерево по BLE: {:?}", props.local_name);
                    
                    // TODO: Здесь мы должны будем прочитать Advertisement Data (Manufacturer Data),
                    // куда мы упакуем наш ID и статус. 
                    // Пока мы просто логируем факт находки.
                }
            }
        }

        // Останавливаем сканирование для экономии батареи
        let _ = self.adapter.stop_scan().await;

        // Пока возвращаем пустой список, так как мы еще не внедрили P2P хендшейк по BLE
        Ok(neighbors)
    }

    async fn send_needle(&self, _to: TreeId, _needle: Needle) -> Result<(), String> {
        // Логика для BLE:
        // 1. Найти `Peripheral` по ID (MAC-адресу), который соответствует `to` (TreeId).
        // 2. Если не подключены — вызвать `peripheral.connect().await`.
        // 3. Вызвать `peripheral.discover_services().await`.
        // 4. Найти характеристику `TAIGA_RX_CHAR_UUID`.
        // 5. Разбить байты (ведь BLE обычно пропускает пакеты только по 20-512 байт за раз - MTU!).
        // 6. Вызвать `peripheral.write(&char, &chunk, WriteType::WithoutResponse).await`.
        
        Err("Отправка Шишек по BLE требует реализации GATT Client".to_string())
    }

    async fn receive_needle(&self) -> Result<(TreeId, Needle), String> {
        // Заглушка. Для приема на мобилках нам понадобится поднять GATT-сервер (Peripheral).
        // Пока вешаем бесконечное ожидание.
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    }

    fn is_connected(&self) -> bool {
        true
    }
}
