use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use std::collections::HashMap;
use taiga_mycelium::{Mycelium, Onion, MeshProxyPayload, TreeId};
use uuid::Uuid;
use std::sync::atomic::{AtomicU64, Ordering};

/// Глобальные атомарные счетчики байт для живого мониторинга трафика
pub static BYTES_SENT_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static BYTES_RECV_TOTAL: AtomicU64 = AtomicU64::new(0);

pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Таблица активных потоков (TCP-туннелей), ключ - stream_id
pub type StreamMap = Arc<Mutex<HashMap<Uuid, tokio::sync::mpsc::Sender<Vec<u8>>>>>;

pub async fn run_socks5_server(
    port: u16,
    mycelium: Arc<Mutex<Mycelium>>,
    local_streams: StreamMap,
    tx_log: std::sync::mpsc::Sender<crate::LogEvent>,
    proxy_enabled: Arc<std::sync::atomic::AtomicBool>,
) {
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            let _ = tx_log.send(crate::LogEvent { level: "PROXY".into(), message: format!("Ошибка запуска SOCKS5: {}", e) });
            return;
        }
    };
    
    let _ = tx_log.send(crate::LogEvent { level: "PROXY".into(), message: format!("Локальный SOCKS5 сервис готовит порт 127.0.0.1:{}", port) });

    // Фоновый процесс регулярного замера скорости каждые 1000 мс
    let log_tx_speed = tx_log.clone();
    let proxy_enabled_speed = proxy_enabled.clone();
    tokio::spawn(async move {
        let mut last_tx = BYTES_SENT_TOTAL.load(Ordering::Relaxed);
        let mut last_rx = BYTES_RECV_TOTAL.load(Ordering::Relaxed);
        
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if !proxy_enabled_speed.load(Ordering::Relaxed) {
                continue;
            }
            
            let cur_tx = BYTES_SENT_TOTAL.load(Ordering::Relaxed);
            let cur_rx = BYTES_RECV_TOTAL.load(Ordering::Relaxed);
            
            let delta_tx = cur_tx.saturating_sub(last_tx);
            let delta_rx = cur_rx.saturating_sub(last_rx);
            
            last_tx = cur_tx;
            last_rx = cur_rx;
            
            if delta_tx > 0 || delta_rx > 0 {
                let tx_fmt = format_bytes(delta_tx);
                let rx_fmt = format_bytes(delta_rx);
                let _ = log_tx_speed.send(crate::LogEvent {
                    level: "SPEED".into(),
                    message: format!("Живая скорость прокси: ↑ {}/с | ↓ {}/с", tx_fmt, rx_fmt),
                });
            }
        }
    });

    loop {
        if let Ok((mut stream, _)) = listener.accept().await {
            if !proxy_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                // Если прокси выключен пользователем в настройках, сбрасываем подключение
                drop(stream);
                continue;
            }

            let m_ref = mycelium.clone();
            let streams_ref = local_streams.clone();
            let stream_id = Uuid::new_v4();
            let log_tx = tx_log.clone();

            tokio::spawn(async move {
                // 1. Простейший SOCKS5 хендшейк
                let mut buf = [0u8; 512];
                if stream.read_exact(&mut buf[0..2]).await.is_err() { return; }
                if buf[0] != 0x05 { return; } // Только SOCKS5
                let nmethods = buf[1] as usize;
                if stream.read_exact(&mut buf[0..nmethods]).await.is_err() { return; }
                
                // Отвечаем: No Auth (0x00)
                if stream.write_all(&[0x05, 0x00]).await.is_err() { return; }

                // 2. Читаем запрос на подключение
                if stream.read_exact(&mut buf[0..4]).await.is_err() { return; }
                if buf[1] != 0x01 { return; } // Поддерживаем только CONNECT (0x01)
                
                let host = match buf[3] {
                    0x01 => { // IPv4
                        if stream.read_exact(&mut buf[0..4]).await.is_err() { return; }
                        format!("{}.{}.{}.{}", buf[0], buf[1], buf[2], buf[3])
                    }
                    0x03 => { // Доменное имя
                        if stream.read_exact(&mut buf[0..1]).await.is_err() { return; }
                        let len = buf[0] as usize;
                        if stream.read_exact(&mut buf[0..len]).await.is_err() { return; }
                        String::from_utf8_lossy(&buf[0..len]).to_string()
                    }
                    _ => return, // IPv6 не поддерживаем для простоты
                };
                
                if stream.read_exact(&mut buf[0..2]).await.is_err() { return; }
                let target_port = u16::from_be_bytes([buf[0], buf[1]]);

                // Отвечаем успехом SOCKS5 (0x00)
                if stream.write_all(&[0x05, 0x00, 0x00, 0x01, 0,0,0,0, 0,0]).await.is_err() { return; }

                let _ = log_tx.send(crate::LogEvent { level: "PROXY".into(), message: format!("Перехвачен запрос к {}:{}. Расчет оптимального пути в Mesh-сети...", host, target_port) });

                // Ищем лучшую Экзит-ноду (с максимальным FreedomLevel с учетом штрафа за расстояние)
                let (exit_node_id, exit_freedom, exit_hops) = {
                    let m = m_ref.lock().await;
                    let mut best_nodes = Vec::new();
                    let mut best_score = -1000;

                    // Смотрим в таблицу маршрутизации
                    for (id, (route, _)) in &m.routing_table.entries {
                        let f = route.target_info.freedom;
                        let hops = route.path.len() as i32;
                        
                        let f_score = match f {
                            taiga_mycelium::FreedomLevel::Full => 34,
                            taiga_mycelium::FreedomLevel::Normal => 30,
                            taiga_mycelium::FreedomLevel::WhitelistOnly => 15,
                            taiga_mycelium::FreedomLevel::None => 0,
                        };
                        
                        let mut score = f_score - hops;

                        if route.target_info.is_virtual_uplink {
                            score -= 10;
                        }

                        if score > best_score {
                            best_score = score;
                            best_nodes.clear();
                            best_nodes.push((*id, f, hops));
                        } else if score == best_score {
                            best_nodes.push((*id, f, hops));
                        }
                    }
                    
                    if !best_nodes.is_empty() {
                        let random_val = uuid::Uuid::new_v4().as_u128() as usize;
                        let idx = random_val % best_nodes.len();
                        let (id, f, h) = best_nodes[idx];
                        (Some(id), f, h)
                    } else {
                        (None, taiga_mycelium::FreedomLevel::None, 0)
                    }
                };

                let target_tree = match exit_node_id {
                    Some(id) => id,
                    None => {
                        let _ = log_tx.send(crate::LogEvent { level: "PROXY".into(), message: "ОШИБКА: Нет доступных Экзит-нод в Mesh-сети для выхода в интернет!".to_string() });
                        return;
                    }
                };

                let freedom_str = match exit_freedom {
                    taiga_mycelium::FreedomLevel::Full => "Полный (VPN)",
                    taiga_mycelium::FreedomLevel::Normal => "Обычный (Normal)",
                    taiga_mycelium::FreedomLevel::WhitelistOnly => "Белые списки",
                    taiga_mycelium::FreedomLevel::None => "Локальный",
                };

                let _ = log_tx.send(crate::LogEvent { 
                    level: "PROXY".into(), 
                    message: format!("Маршрут найден! Экзит-нода: {} (Свобода: {}, Дистанция: {} хоп, Скорость: BLE ~0.6КБ/с / P2P ~58МБ/с)", target_tree, freedom_str, exit_hops) 
                });

                // Создаем канал для получения ответов из Mesh-сети
                let (tx, mut rx) = mpsc::channel::<Vec<u8>>(1000);
                streams_ref.lock().await.insert(stream_id, tx);

                // Отправляем Connect пакет через Mesh
                let connect_payload = MeshProxyPayload::Connect { stream_id, host, port: target_port };
                let _ = send_mesh_payload(&m_ref, target_tree, connect_payload).await;

                // Разделяем TCP сокет браузера на чтение и запись
                let (mut reader, mut writer) = stream.into_split();

                // Читаем из браузера -> Шлем в Mesh-сеть
                let m_ref_reader = m_ref.clone();
                let reader_task = tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    loop {
                        match reader.read(&mut buf).await {
                            Ok(0) => break, // EOF
                            Ok(n) => {
                                BYTES_SENT_TOTAL.fetch_add(n as u64, Ordering::Relaxed);
                                let payload = MeshProxyPayload::Data { stream_id, data: buf[0..n].to_vec() };
                                let _ = send_mesh_payload(&m_ref_reader, target_tree, payload).await;
                            }
                            Err(_) => break,
                        }
                    }
                    // Закрываем поток
                    let _ = send_mesh_payload(&m_ref_reader, target_tree, MeshProxyPayload::Close { stream_id }).await;
                });

                // Читаем из Mesh-сети -> Пишем в браузер
                let writer_task = tokio::spawn(async move {
                    while let Some(data) = rx.recv().await {
                        BYTES_RECV_TOTAL.fetch_add(data.len() as u64, Ordering::Relaxed);
                        if writer.write_all(&data).await.is_err() {
                            break;
                        }
                    }
                });

                // Ждем завершения
                let _ = tokio::join!(reader_task, writer_task);
                streams_ref.lock().await.remove(&stream_id);
                let _ = log_tx.send(crate::LogEvent { level: "PROXY".into(), message: format!("Поток {} закрыт.", stream_id) });
            });
        }
    }
}

/// Утилита для заворачивания MeshProxyPayload в Onion и отправки
pub async fn send_mesh_payload(m_ref: &Arc<Mutex<Mycelium>>, target_id: TreeId, payload: MeshProxyPayload) -> Result<(), String> {
    let payload_bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    
    let m = m_ref.lock().await;
    let path = match m.routing_table.get_path(&target_id) {
        Some(p) => p,
        None => return Err("Маршрут не найден".to_string()),
    };

    let core = Onion::Core { sender: m.local_info.id, payload: payload_bytes };
    let mut current_data = serde_json::to_vec(&core).map_err(|e| e.to_string())?;

    for i in (0..path.len()).rev() {
        let node_id = path[i];
        let node_info = m.routing_table.get_info(&node_id)
            .or_else(|| m.known_nodes.get(&node_id).cloned())
            .ok_or("Неизвестный узел в маршруте")?;
        let pub_key_bytes = <[u8; 32]>::try_from(node_info.public_key.as_slice())
            .map_err(|_| "Неверная длина публичного ключа у узла".to_string())?;
        let pub_key = x25519_dalek::PublicKey::from(pub_key_bytes);

        let encrypted = m.crypto.encrypt(&pub_key, &current_data)?;

        if i > 0 {
            let layer = Onion::Layer { next_hop: node_id, encrypted_data: encrypted };
            current_data = serde_json::to_vec(&layer).map_err(|e| e.to_string())?;
        } else {
            current_data = encrypted;
        }
    }
    
    let next_hop = path[0];
    let needles = taiga_resin::split_into_needles(&current_data, next_hop, 200);

    for needle in needles {
        m.broadcast_needle(next_hop, needle).await;
    }
    Ok(())
}

/// Выполняется на стороне Экзит-ноды. Отвечает за прием команд из Mesh и пересылку в реальный интернет.
pub async fn handle_exit_node_request(
    payload: MeshProxyPayload, 
    sender_id: TreeId, 
    m_ref: Arc<Mutex<Mycelium>>, 
    exit_streams: StreamMap,
    tx_log: std::sync::mpsc::Sender<crate::LogEvent>,
) {
    match payload {
        MeshProxyPayload::Connect { stream_id, host, port } => {
            let _ = tx_log.send(crate::LogEvent { level: "EXIT".into(), message: format!("Открываем TCP {}:{} для узла {}", host, port, sender_id) });
            match TcpStream::connect((host.as_str(), port)).await {
                Ok(stream) => {
                    let (mut reader, mut writer) = stream.into_split();
                    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(1000);
                    exit_streams.lock().await.insert(stream_id, tx);

                    let exit_streams_clean = exit_streams.clone();
                    let log_tx = tx_log.clone();
                    let m_ref_clean = m_ref.clone();
                    let m_ref_close = m_ref.clone();

                    tokio::spawn(async move {
                        let writer_task = tokio::spawn(async move {
                            while let Some(data) = rx.recv().await {
                                if writer.write_all(&data).await.is_err() { break; }
                            }
                        });

                        let reader_task = tokio::spawn(async move {
                            let mut buf = [0u8; 1024];
                            loop {
                                match reader.read(&mut buf).await {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        let mesh_payload = MeshProxyPayload::Data { stream_id, data: buf[0..n].to_vec() };
                                        if send_mesh_payload(&m_ref_clean, sender_id, mesh_payload).await.is_err() {
                                            break;
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                        });

                        tokio::select! {
                            _ = writer_task => {},
                            _ = reader_task => {},
                        }

                        exit_streams_clean.lock().await.remove(&stream_id);
                        let _ = log_tx.send(crate::LogEvent { level: "EXIT".into(), message: format!("Поток {} закрыт", stream_id) });
                        let _ = send_mesh_payload(&m_ref_close, sender_id, MeshProxyPayload::Close { stream_id }).await;
                    });
                }
                Err(e) => {
                    let _ = tx_log.send(crate::LogEvent { level: "EXIT".into(), message: format!("Ошибка соединения с {}:{}: {}", host, port, e) });
                    let _ = send_mesh_payload(&m_ref, sender_id, MeshProxyPayload::Close { stream_id }).await;
                }
            }
        }
        MeshProxyPayload::Data { stream_id, data } => {
            let tx = {
                let streams = exit_streams.lock().await;
                streams.get(&stream_id).cloned()
            };
            if let Some(tx) = tx {
                let _ = tx.send(data).await;
            }
        }
        MeshProxyPayload::Close { stream_id } => {
            exit_streams.lock().await.remove(&stream_id);
            let _ = tx_log.send(crate::LogEvent { level: "EXIT".into(), message: format!("Поток {} закрыт клиентом", stream_id) });
        }
    }
}
