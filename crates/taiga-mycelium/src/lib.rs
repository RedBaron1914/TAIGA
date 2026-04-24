use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::collections::{HashSet, HashMap};

pub mod udp_root;
pub mod ble_root;
pub mod wifi_root;
pub mod crypto;
pub mod dtn;

#[cfg(target_os = "android")]
pub mod jni_bridge;

use crypto::CryptoModule;
use dtn::DtnBuffer;

/// Уникальный идентификатор любого устройства (Дерева или Просвета)
pub type TreeId = Uuid;

/// Статус узла сети (Node Status)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    /// Дерево — обычный пользователь без интернета, передающий трафик дальше.
    Tree,
    /// Просвет — статичная экзит-нода (ПК с Wi-Fi или телефон на подоконнике с LTE).
    Clearing,
    /// Проводник — мобильная экзит-нода (человек с работающим интернетом в движении).
    Ranger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FreedomLevel {
    None,          // Только локальная Mesh-сеть (интернета нет)
    WhitelistOnly, // Жесткие гос. белые списки (работают только крупные сайты типа ya.ru)
    Normal,        // Обычный интернет вне белых списков (работает google.com, сайты колледжей, но Discord заблокирован)
    Full,          // Полный доступ без цензуры (включен VPN, работает Discord/YouTube)
}

/// Метаданные Дерева (пользователя), которые видят соседи
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TreeInfo {
    pub id: TreeId,
    pub status: NodeStatus,
    /// Публичный ключ для End-to-End шифрования (Шишек)
    pub public_key: Vec<u8>, 
    /// Уровень свободы выхода в глобальную сеть
    pub freedom: FreedomLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteUpdate {
    pub target_info: TreeInfo,
    pub path: Vec<TreeId>, // Путь до цели: [NextHop, ..., Target]
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MeshProxyPayload {
    /// Запрос от клиента к Экзит-ноде: Открой TCP-соединение до этого хоста и порта
    Connect { stream_id: u32, host: String, port: u16 },
    /// Трансляция байт в обе стороны
    Data { stream_id: u32, data: Vec<u8> },
    /// Закрытие соединения
    Close { stream_id: u32 },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Onion {
    /// Конечный слой: Содержит ID отправителя и сами данные (теперь это может быть и TCP-трафик, и системный сигнал)
    Core { sender: TreeId, payload: Vec<u8> },
    /// Транзитный слой: Кому переслать дальше, и зашифрованный кусок для него
    Layer { next_hop: TreeId, encrypted_data: Vec<u8> },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MeshPayload {
    /// Запрос на открытие TCP-соединения на Exit Node
    Connect { stream_id: u32, host: String, port: u16 },
    /// Данные для существующего потока
    Data { stream_id: u32, data: Vec<u8> },
    /// Закрытие потока
    Close { stream_id: u32 },
}

/// Фрагментированный пакет данных (Хвоинка).
/// Получается после работы мультиплексора (taiga-resin), 
/// который разрезает большую Шишку (Cone).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Needle {
    /// ID оригинального большого пакета (Шишки)
    pub cone_id: Uuid,
    /// Порядковый номер этой Хвоинки
    pub sequence_number: u32,
    /// Общее количество Хвоинок в этой Шишке
    pub total_needles: u32,
    /// Сырые (зашифрованные) данные этого куска
    pub payload: Vec<u8>,
    /// Целевой узел (Экзит-нода или получатель в сети)
    pub target_tree: TreeId,
}

/// Абстракция для любого типа соединения (Корень).
/// Это может быть Bluetooth LE, Wi-Fi Direct, локальный UDP сокет для тестов.
#[async_trait]
pub trait Root: Send + Sync {
    /// Уникальный идентификатор соединения (например, MAC-адрес)
    fn id(&self) -> String;

    /// Начать поиск новых соседей (Пустить корни)
    async fn discover(&self, local_routes: Vec<RouteUpdate>) -> Result<Vec<(TreeInfo, Vec<RouteUpdate>)>, String>;

    /// Обновить метаданные о себе для отправки при знакомстве (включая маршруты)
    async fn update_local_info(&self, info: TreeInfo);

    /// Отправить фрагмент данных (Хвоинку) конкретному соседу
    async fn send_needle(&self, to: TreeId, needle: Needle) -> Result<(), String>;

    /// Получить входящую Хвоинку от соседей (асинхронный стрим/ожидание)
    async fn receive_needle(&self) -> Result<(TreeId, Needle), String>;
    
    /// Текущее состояние подключения
    fn is_connected(&self) -> bool;
}

/// Таблица маршрутизации (Routing Table)
#[derive(Debug, Clone)]
pub struct RoutingTable {
    pub entries: HashMap<TreeId, RouteUpdate>,
}

impl RoutingTable {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Обновление таблицы при получении инфы от соседа
    pub fn update_from_neighbor(&mut self, local_id: TreeId, neighbor_info: TreeInfo, neighbor_routes: &[RouteUpdate]) {
        let neighbor_id = neighbor_info.id;
        
        // Добавляем самого соседа (расстояние 1, путь только он сам)
        self.entries.insert(neighbor_id, RouteUpdate {
            target_info: neighbor_info,
            path: vec![neighbor_id],
        });

        // Анализируем, кого знает сосед
        for route in neighbor_routes {
            let target_id = route.target_info.id;
            if target_id == local_id { continue; } // Себя не добавляем
            if route.path.contains(&local_id) { continue; } // Защита от маршрутных петель
            
            let mut new_path = vec![neighbor_id];
            new_path.extend_from_slice(&route.path);
            
            if let Some(current_route) = self.entries.get(&target_id) {
                let new_freedom = route.target_info.freedom;
                let current_freedom = current_route.target_info.freedom;
                
                let is_better = new_freedom > current_freedom || 
                                (new_freedom == current_freedom && new_path.len() < current_route.path.len());

                if is_better {
                    // Нашли более свободный ИЛИ более короткий путь при равной свободе
                    self.entries.insert(target_id, RouteUpdate {
                        target_info: route.target_info.clone(),
                        path: new_path,
                    });
                }
            } else {
                // Мы вообще не знали об этом узле
                self.entries.insert(target_id, RouteUpdate {
                    target_info: route.target_info.clone(),
                    path: new_path,
                });
            }
        }
    }
    
    pub fn get_next_hop(&self, target_id: &TreeId) -> Option<TreeId> {
        self.entries.get(target_id).and_then(|r| r.path.first().copied())
    }
    
    pub fn get_path(&self, target_id: &TreeId) -> Option<Vec<TreeId>> {
        self.entries.get(target_id).map(|r| r.path.clone())
    }
    
    pub fn get_info(&self, target_id: &TreeId) -> Option<TreeInfo> {
        self.entries.get(target_id).map(|r| r.target_info.clone())
    }
}

/// Главная структура P2P-сети (Мицелий).
/// Хранит граф связей и управляет Корнями.
pub struct Mycelium {
    pub local_info: TreeInfo,
    /// Активные соединения (Bluetooth, Wi-Fi, Симуляции)
    pub roots: Vec<Box<dyn Root>>,
    /// Известные соседи в радиусе одного прыжка
    pub neighbors: HashSet<TreeId>,
    /// Таблица маршрутизации
    pub routing_table: RoutingTable,
    /// Кэш метаданных всех известных узлов в сети
    pub known_nodes: HashMap<TreeId, TreeInfo>,
    /// Модуль криптографии (Ключи и шифрование)
    pub crypto: CryptoModule,
    /// Транзитный буфер (Store-and-Forward)
    pub dtn: Option<DtnBuffer>,
}

impl Mycelium {
    pub fn new(id: TreeId, status: NodeStatus) -> Self {
        let crypto = CryptoModule::new();
        
        Self {
            local_info: TreeInfo {
                id,
                status,
                public_key: crypto.public_key.as_bytes().to_vec(),
                freedom: FreedomLevel::None,
            },
            roots: Vec::new(),
            neighbors: HashSet::new(),
            routing_table: RoutingTable::new(),
            known_nodes: HashMap::new(),
            crypto,
            dtn: None,
        }
    }

    /// Инициализирует буфер DTN на диске
    pub fn init_dtn<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<(), String> {
        let dtn = DtnBuffer::new(path)?;
        self.dtn = Some(dtn);
        Ok(())
    }

    /// Подключить новый интерфейс связи (Пустить корень)
    pub fn attach_root(&mut self, root: Box<dyn Root>) {
        self.roots.push(root);
    }

    /// Отправить Хвоинку через все доступные интерфейсы (Multihoming)
    pub async fn broadcast_needle(&self, to: TreeId, needle: Needle) {
        for root in &self.roots {
            let _ = root.send_needle(to, needle.clone()).await;
        }
    }
    
    /// Получить локальные маршруты для отправки соседям
    pub fn get_local_routes(&self) -> Vec<RouteUpdate> {
        self.routing_table.entries.values().cloned().collect()
    }

    /// Сбросить Кору: сгенерировать новый эфемерный ID для анонимности
    pub async fn rotate_bark(&mut self) {
        let new_id = Uuid::new_v4();
        self.local_info.id = new_id;
        
        // Ротируем ключи шифрования
        let new_pub_key = self.crypto.rotate_keys();
        self.local_info.public_key = new_pub_key.as_bytes().to_vec();
        
        // Очищаем графы и таблицы, так как для сети мы теперь новый узел
        self.neighbors.clear();
        self.routing_table = RoutingTable::new();
        self.known_nodes.clear();
        
        let updated_info = self.local_info.clone();
        for root in &self.roots {
            root.update_local_info(updated_info.clone()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routing_table() {
        let mut table = RoutingTable::new();
        let local_id = Uuid::new_v4();
        
        let neighbor_id = Uuid::new_v4();
        let neighbor_info = TreeInfo {
            id: neighbor_id,
            status: NodeStatus::Tree,
            public_key: vec![],
            freedom: FreedomLevel::None,
        };
        
        let target_id = Uuid::new_v4();
        let routes = vec![
            RouteUpdate {
                target_info: TreeInfo {
                    id: target_id,
                    status: NodeStatus::Tree,
                    public_key: vec![],
                    freedom: FreedomLevel::None,
                },
                path: vec![neighbor_id, target_id],
            }
        ];
        
        // Обновляем от соседа
        table.update_from_neighbor(local_id, neighbor_info, &routes);
        
        // Проверяем, что сосед есть в таблице (маршрут к нему прямо)
        let neighbor_hop = table.get_next_hop(&neighbor_id).unwrap();
        assert_eq!(neighbor_hop, neighbor_id);
        
        // Проверяем, что дальняя цель тоже есть, и прыжок идет через соседа
        let next_hop_target = table.get_next_hop(&target_id).unwrap();
        assert_eq!(next_hop_target, neighbor_id);
        
        let path = table.get_path(&target_id).unwrap();
        assert_eq!(path.len(), 3);
        assert_eq!(path[0], neighbor_id);
        assert_eq!(path[1], neighbor_id);
        assert_eq!(path[2], target_id);
    }

    #[test]
    fn test_freedom_level_priority() {
        let mut table = RoutingTable::new();
        let local_id = Uuid::new_v4();
        let target_id = Uuid::new_v4();
        
        let neighbor_1 = Uuid::new_v4();
        let neighbor_2 = Uuid::new_v4();

        // Узел 1 предлагает короткий маршрут, но без свободы
        let n1_info = TreeInfo {
            id: neighbor_1, status: NodeStatus::Tree, public_key: vec![], freedom: FreedomLevel::None,
        };
        let routes_1 = vec![RouteUpdate {
            target_info: TreeInfo { id: target_id, status: NodeStatus::Tree, public_key: vec![], freedom: FreedomLevel::None },
            path: vec![neighbor_1, target_id],
        }];
        table.update_from_neighbor(local_id, n1_info, &routes_1);
        assert_eq!(table.get_next_hop(&target_id), Some(neighbor_1));

        // Узел 2 предлагает длинный маршрут, но с полной свободой (Full)
        let n2_info = TreeInfo {
            id: neighbor_2, status: NodeStatus::Ranger, public_key: vec![], freedom: FreedomLevel::Full,
        };
        let routes_2 = vec![RouteUpdate {
            target_info: TreeInfo { id: target_id, status: NodeStatus::Ranger, public_key: vec![], freedom: FreedomLevel::Full },
            path: vec![neighbor_2, Uuid::new_v4(), target_id],
        }];
        table.update_from_neighbor(local_id, n2_info, &routes_2);

        // Маршрут должен переключиться на Узел 2, так как свобода важнее короткого пути
        assert_eq!(table.get_next_hop(&target_id), Some(neighbor_2));
    }
}
