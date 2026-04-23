use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use std::collections::HashMap;
use uuid::Uuid;
use taiga_mycelium::{Mycelium, Onion, MeshProxyPayload, TreeId};

/// Таблица активных потоков (TCP-туннелей), ключ - stream_id
pub type StreamMap = Arc<Mutex<HashMap<u32, tokio::sync::mpsc::Sender<Vec<u8>>>>>;

pub async fn run_socks5_server(
    port: u16,
    mycelium: Arc<Mutex<Mycelium>>,
    local_streams: StreamMap,
    tx_log: std::sync::mpsc::Sender<crate::LogEvent>,
) {
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            let _ = tx_log.send(crate::LogEvent { level: "PROXY".into(), message: format!("Ошибка запуска SOCKS5: {}", e) });
            return;
        }
    };
    
    let mut next_stream_id = 1u32;
    let _ = tx_log.send(crate::LogEvent { level: "PROXY".into(), message: format!("Локальный SOCKS5 прокси запущен на 127.0.0.1:{}", port) });

    loop {
        if let Ok((mut stream, _)) = listener.accept().await {
            let m_ref = mycelium.clone();
            let streams_ref = local_streams.clone();
            let stream_id = next_stream_id;
            next_stream_id += 1;
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

                let _ = log_tx.send(crate::LogEvent { level: "PROXY".into(), message: format!("Пойман трафик на {}:{}. Ищем Экзит-ноду...", host, target_port) });

                // Ищем лучшую Экзит-ноду (с максимальным FreedomLevel)
                let exit_node_id = {
                    let m = m_ref.lock().await;
                    let mut best_node = None;
                    let mut best_freedom = taiga_mycelium::FreedomLevel::None;

                    // Смотрим в таблицу маршрутизации, кто имеет наибольшую свободу
                    for (id, route) in &m.routing_table.entries {
                        let f = route.target_info.freedom;
                        // Сравниваем Enum (упрощенно: Full > Normal > WhitelistOnly > None)
                        let f_score = match f {
                            taiga_mycelium::FreedomLevel::Full => 4,
                            taiga_mycelium::FreedomLevel::Normal => 3,
                            taiga_mycelium::FreedomLevel::WhitelistOnly => 2,
                            taiga_mycelium::FreedomLevel::None => 1,
                        };
                        let best_score = match best_freedom {
                            taiga_mycelium::FreedomLevel::Full => 4,
                            taiga_mycelium::FreedomLevel::Normal => 3,
                            taiga_mycelium::FreedomLevel::WhitelistOnly => 2,
                            taiga_mycelium::FreedomLevel::None => 1,
                        };

                        if f_score > best_score {
                            best_freedom = f;
                            best_node = Some(*id);
                        }
                    }
                    best_node
                };

                let target_tree = match exit_node_id {
                    Some(id) => id,
                    None => {
                        let _ = log_tx.send(crate::LogEvent { level: "PROXY".into(), message: format!("Нет доступных Экзит-нод для маршрутизации!") });
                        return;
                    }
                };

                let _ = log_tx.send(crate::LogEvent { level: "PROXY".into(), message: format!("Маршрут найден! Экзит-нода: {}", target_tree) });

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
        let node_info = m.known_nodes.get(&node_id).ok_or("Неизвестный узел в маршруте")?;
        let pub_key = x25519_dalek::PublicKey::from(
            <[u8; 32]>::try_from(node_info.public_key.as_slice()).unwrap()
        );

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

    if let Some(root) = m.roots.first() {
        for needle in needles {
            let _ = root.send_needle(next_hop, needle).await;
        }
        Ok(())
    } else {
        Err("Нет активных Корней".to_string())
    }
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

                    // Читаем из Mesh -> Пишем в реальный TCP
                    tokio::spawn(async move {
                        while let Some(data) = rx.recv().await {
                            if writer.write_all(&data).await.is_err() { break; }
                        }
                    });

                    // Читаем из реального TCP -> Шлем в Mesh
                    let m_ref_reader = m_ref.clone();
                    let log_tx = tx_log.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 1024];
                        loop {
                            match reader.read(&mut buf).await {
                                Ok(0) => break,
                                Ok(n) => {
                                    let mesh_payload = MeshProxyPayload::Data { stream_id, data: buf[0..n].to_vec() };
                                    let _ = send_mesh_payload(&m_ref_reader, sender_id, mesh_payload).await;
                                }
                                Err(_) => break,
                            }
                        }
                        let _ = log_tx.send(crate::LogEvent { level: "EXIT".into(), message: format!("Поток {} закрыт удаленным сервером", stream_id) });
                        let _ = send_mesh_payload(&m_ref_reader, sender_id, MeshProxyPayload::Close { stream_id }).await;
                    });
                }
                Err(e) => {
                    let _ = tx_log.send(crate::LogEvent { level: "EXIT".into(), message: format!("Ошибка соединения с {}:{}: {}", host, port, e) });
                    let _ = send_mesh_payload(&m_ref, sender_id, MeshProxyPayload::Close { stream_id }).await;
                }
            }
        }
        MeshProxyPayload::Data { stream_id, data } => {
            let mut streams = exit_streams.lock().await;
            if let Some(tx) = streams.get_mut(&stream_id) {
                let _ = tx.send(data).await;
            }
        }
        MeshProxyPayload::Close { stream_id } => {
            exit_streams.lock().await.remove(&stream_id);
            let _ = tx_log.send(crate::LogEvent { level: "EXIT".into(), message: format!("Поток {} закрыт клиентом", stream_id) });
        }
    }
}
