use eframe::egui;
use std::sync::Arc;
use tokio::sync::Mutex;
use taiga_mycelium::{Mycelium, NodeStatus, TreeInfo, Root, Onion};
use taiga_mycelium::udp_root::UdpRoot;
use taiga_resin::ResinAssembler;
use uuid::Uuid;

pub mod proxy;

#[derive(Clone)]
pub struct LogEvent {
    pub level: String,
    pub message: String,
}

#[derive(Clone, PartialEq)]
pub enum Tab {
    Dashboard,
    Logs,
    Settings,
}

pub struct TaigaApp {
    mycelium: Arc<Mutex<Mycelium>>,
    logs: Vec<LogEvent>,
    local_info: Option<TreeInfo>,
    rx: std::sync::mpsc::Receiver<LogEvent>,
    routes: Vec<(Uuid, Uuid, u32, taiga_mycelium::FreedomLevel)>, // Target, NextHop, Hops, Freedom
    proxy_enabled: bool,
    current_tab: Tab,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<String>,
}

impl TaigaApp {
    pub fn new(cc: &eframe::CreationContext<'_>, app_data_dir: std::path::PathBuf) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let (tx, rx) = std::sync::mpsc::channel();
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let port: u16 = (rand::random::<u16>() % 21) + 40000; 

        #[cfg(target_os = "android")]
        let id = taiga_mycelium::jni_bridge::get_android_node_id().unwrap_or_else(Uuid::new_v4);
        #[cfg(not(target_os = "android"))]
        let id = Uuid::new_v4();

        let mut m = Mycelium::new(id, NodeStatus::Tree);
        
        let _ = std::fs::create_dir_all(&app_data_dir);
        let dtn_path = app_data_dir.join(format!("taiga_dtn_{}.redb", id));
        if let Err(e) = m.init_dtn(&dtn_path) {
            log::error!("Не удалось инициализировать DTN буфер: {}", e);
        }
        
        let assembler = Arc::new(Mutex::new(ResinAssembler::new()));
        let mycelium_ref = Arc::new(Mutex::new(m));
        
        let m_for_spawn = mycelium_ref.clone();
        let assembler_clone = assembler.clone();
        let ctx = cc.egui_ctx.clone();

        let tx_log = tx.clone();
        let log_msg = move |level: &str, msg: &str| {
            let _ = tx_log.send(LogEvent { level: level.to_string(), message: msg.to_string() });
        };
        
        log_msg("SYSTEM", "Инициализация ядра TAIGA...");

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let local_info = {
                    let m_guard = m_for_spawn.lock().await;
                    m_guard.local_info.clone()
                };

                let (needle_agg_tx, mut needle_agg_rx) = tokio::sync::mpsc::channel::<(Uuid, taiga_mycelium::Needle)>(1000);

                #[cfg(target_os = "android")]
                {
                    let (sync_tx, sync_rx) = std::sync::mpsc::channel();
                    taiga_mycelium::jni_bridge::set_jni_sender(sync_tx);
                    
                    let m_for_jni = m_for_spawn.clone();
                    let tx_for_jni = tx.clone();
                    let ctx_for_jni = ctx.clone();
                    let needle_agg_tx_jni = needle_agg_tx.clone();
                    
                    let local_info_ble = m_for_jni.lock().await.local_info.clone();
                    let ble_root = taiga_mycelium::ble_root::BleRoot::new(local_info_ble);
                    
                    // Добавляем BleRoot в список корней (клонируем)
                    m_for_jni.lock().await.attach_root(Arc::new(ble_root.clone()));
                    
                    // Слушатель для BleRoot
                    let needle_tx_ble = needle_agg_tx.clone();
                    let ble_root_listener = ble_root.clone();
                    tokio::spawn(async move {
                        while let Ok(res) = ble_root_listener.receive_needle().await {
                            let _ = needle_tx_ble.send(res).await;
                        }
                    });

                    tokio::spawn(async move {
                        loop {
                            if let Ok(event) = sync_rx.try_recv() {
                                match event {
                                    taiga_mycelium::jni_bridge::JniEvent::WifiDirectConnected { ip, is_group_owner } => {
                                        let _ = tx_for_jni.send(LogEvent {
                                            level: "WIFI".to_string(),
                                            message: format!("Wi-Fi Direct канал установлен! IP: {}, GO: {}", ip, is_group_owner),
                                        });
                                        ctx_for_jni.request_repaint();
                                        
                                        let local = m_for_jni.lock().await.local_info.clone();
                                        if let Ok(wifi_root) = taiga_mycelium::wifi_root::WifiRoot::new(local, ip, is_group_owner).await {
                                            let root_arc: Arc<dyn Root> = Arc::new(wifi_root.clone());
                                            m_for_jni.lock().await.attach_root(root_arc);
                                            
                                            // Слушатель для нового Wi-Fi корня
                                            let needle_tx = needle_agg_tx_jni.clone();
                                            tokio::spawn(async move {
                                                while let Ok(res) = wifi_root.receive_needle().await {
                                                    let _ = needle_tx.send(res).await;
                                                }
                                            });
                                        }
                                    },
                                    taiga_mycelium::jni_bridge::JniEvent::BleDeviceDiscovered(mac, id_bytes) => {
                                        if let Ok(id) = Uuid::from_slice(&id_bytes) {
                                            ble_root.add_discovered_neighbor(mac, id).await;
                                        }
                                    },
                                    taiga_mycelium::jni_bridge::JniEvent::BleMessageReceived(mac, payload) => {
                                        ble_root.inject_needle(mac, payload).await;
                                    },
                                    _ => {}
                                }
                            } else {
                                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            }
                        }
                    });
                }

                let local_streams: proxy::StreamMap = Arc::new(Mutex::new(std::collections::HashMap::new()));

                if let Ok(udp_root) = UdpRoot::new(port, local_info).await {
                    let _rx_root = udp_root.clone();
                    let m_for_rx = m_for_spawn.clone();
                    let tx_for_rx = tx.clone();
                    let ctx_for_rx = ctx.clone();
                    
                    let tx_for_rx_closure = tx_for_rx.clone();
                    let ctx_for_rx_closure = ctx_for_rx.clone();
                    let log_rx = move |level: &str, msg: &str| {
                        let _ = tx_for_rx_closure.send(LogEvent { level: level.to_string(), message: msg.to_string() });
                        ctx_for_rx_closure.request_repaint();
                    };
                    
                    log_rx("NETWORK", &format!("UDP Транспорт запущен на порту {}", port));

                    // Слушатель для UDP корня
                    let needle_tx_udp = needle_agg_tx.clone();
                    let udp_root_clone = udp_root.clone();
                    tokio::spawn(async move {
                        while let Ok(res) = udp_root_clone.receive_needle().await {
                            let _ = needle_tx_udp.send(res).await;
                        }
                    });
                    
                    let exit_streams: proxy::StreamMap = Arc::new(Mutex::new(std::collections::HashMap::new()));
                    
                    let assembler_for_rx = assembler_clone.clone();
                    let local_streams_for_rx = local_streams.clone();
                    tokio::spawn(async move {
                        let mut seen_needles = std::collections::HashSet::new();
                        loop {
                            if let Some((sender_id, needle)) = needle_agg_rx.recv().await {
                                let needle_id = (needle.cone_id, needle.sequence_number);
                                if !seen_needles.insert(needle_id) { continue; }
                                if seen_needles.len() > 10000 { seen_needles.clear(); }

                                let m_lock = m_for_rx.lock().await;
                                let local_id = m_lock.local_info.id;
                                
                                if needle.target_tree == Uuid::nil() {
                                    m_lock.broadcast_needle(Uuid::nil(), needle.clone()).await;
                                    drop(m_lock);
                                    
                                    let mut asm = assembler_for_rx.lock().await;
                                    if let Some(full_payload) = asm.receive_needle(needle)
                                        && let Ok(text) = String::from_utf8(full_payload) {
                                            log_rx("GOSSIP", &format!("Шёпот от {}: {}", sender_id.to_string().chars().take(8).collect::<String>(), text));
                                        }
                                } else if needle.target_tree == local_id {
                                    drop(m_lock);
                                    let mut asm = assembler_for_rx.lock().await;
                                    if let Some(full_encrypted_payload) = asm.receive_needle(needle) {
                                        let decrypted = {
                                            let m = m_for_rx.lock().await;
                                            m.crypto.decrypt(&full_encrypted_payload)
                                        };

                                        if let Ok(decrypted_bytes) = decrypted
                                            && let Ok(onion) = serde_json::from_slice::<Onion>(&decrypted_bytes) {
                                                match onion {
                                                    Onion::Core { sender, payload } => {
                                                        if let Ok(mesh_payload) = serde_json::from_slice::<taiga_mycelium::MeshProxyPayload>(&payload) {
                                                            let m_ref_exit = m_for_rx.clone();
                                                            let exit_streams_ref = exit_streams.clone();
                                                            let local_streams_ref = local_streams_for_rx.clone();
                                                            let tx_log_exit = tx_for_rx.clone();
                                                            tokio::spawn(async move {
                                                                match mesh_payload {
                                                                    taiga_mycelium::MeshProxyPayload::Data { stream_id, data } => {
                                                                        let mut locals = local_streams_ref.lock().await;
                                                                        if let Some(tx) = locals.get_mut(&stream_id) {
                                                                            let _ = tx.send(data).await;
                                                                            return;
                                                                        }
                                                                        drop(locals);
                                                                        let mut exits = exit_streams_ref.lock().await;
                                                                        if let Some(tx) = exits.get_mut(&stream_id) {
                                                                            let _ = tx.send(data).await;
                                                                        }
                                                                    }
                                                                    taiga_mycelium::MeshProxyPayload::Close { stream_id } => {
                                                                        local_streams_ref.lock().await.remove(&stream_id);
                                                                        exit_streams_ref.lock().await.remove(&stream_id);
                                                                    }
                                                                    _ => {
                                                                        proxy::handle_exit_node_request(mesh_payload, sender, m_ref_exit, exit_streams_ref, tx_log_exit).await;
                                                                    }
                                                                }
                                                            });
                                                        } else if let Ok(_text) = String::from_utf8(payload) {
                                                            log_rx("DELIVERY", &format!("Доставлен пакет от {}", sender));
                                                        }
                                                    },
                                                    Onion::Layer { next_hop, encrypted_data } => {
                                                        let m_guard = m_for_rx.lock().await;
                                                        if let Some(path) = m_guard.routing_table.get_path(&next_hop) {
                                                            let actual_next_hop = path[0];
                                                            log_rx("ROUTING", &format!("Снят слой луковицы. Пересылка к {}", actual_next_hop));
                                                            let needles = taiga_resin::split_into_needles(&encrypted_data, actual_next_hop, 200);
                                                            for n in needles {
                                                                m_guard.broadcast_needle(actual_next_hop, n).await;
                                                            }
                                                        } else {
                                                            log_rx("DTN", &format!("Маршрут к {} потерян. Сохранено в буфер.", next_hop));
                                                            if let Some(dtn) = &m_guard.dtn {
                                                                let _ = dtn.store_transit_packet(next_hop, &encrypted_data);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                    }
                                } else {
                                    // Пакет не нам, игнорируем
                                }
                            }
                        }
                    });

                    let mut m_guard = m_for_spawn.lock().await;
                    m_guard.attach_root(Arc::new(udp_root));
                }
                
                // Запуск локального SOCKS5-прокси сервера
                let m_ref_proxy = m_for_spawn.clone();
                let tx_for_proxy = tx.clone();
                let local_streams_for_proxy = local_streams.clone();
                tokio::spawn(async move {
                    proxy::run_socks5_server(1080, m_ref_proxy, local_streams_for_proxy, tx_for_proxy).await;
                });

                // Фоновый цикл сканирования
                let tx_for_scan = tx.clone();
                let ctx_for_scan = ctx.clone();
                let m_for_freedom = m_for_spawn.clone();
                let tx_for_freedom = tx.clone();
                let ctx_for_freedom = ctx.clone();
                let local_streams_for_freedom = local_streams.clone();

                // Фоновый процесс для определения "Уровня Свободы" (FreedomLevel)
                tokio::spawn(async move {
                    let _client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(3))
                        .build()
                        .unwrap();

                    loop {
                        let mut new_freedom = taiga_mycelium::FreedomLevel::None;
                        
                        #[cfg(target_os = "android")]
                        let has_real_uplink = taiga_mycelium::jni_bridge::has_physical_internet();
                        #[cfg(not(target_os = "android"))]
                        let has_real_uplink = true;

                        let is_using_socks5 = local_streams_for_freedom.lock().await.len() > 0;
                        
                        // 1. Проверяем "Белые списки" (гос. ресурсы, крупные поисковики)
                        let has_whitelist = _client.get("https://ya.ru").send().await.is_ok();
                        
                        if has_whitelist {
                            new_freedom = taiga_mycelium::FreedomLevel::WhitelistOnly;
                            
                            // 2. Проверяем выход за пределы "белых списков" (обычные сайты)
                            let has_normal = _client.get("https://coworking.tyuiu.ru").send().await.is_ok();
                            
                            if has_normal {
                                new_freedom = taiga_mycelium::FreedomLevel::Normal;
                                
                                // 3. Проверяем полный доступ (VPN/Антизапрет)
                                let has_full = _client.get("https://discord.com").send().await.is_ok();
                                
                                if has_full {
                                    new_freedom = taiga_mycelium::FreedomLevel::Full;
                                }
                            }
                        }

                        let mut changed = false;
                        {
                            let mut m_guard = m_for_freedom.lock().await;
                            let is_virtual = !has_real_uplink || is_using_socks5;
                            if m_guard.local_info.freedom != new_freedom || m_guard.local_info.is_virtual_uplink != is_virtual {
                                m_guard.local_info.freedom = new_freedom;
                                m_guard.local_info.is_virtual_uplink = is_virtual;
                                changed = true;
                            }
                        }

                        if changed {
                            let _ = tx_for_freedom.send(LogEvent {
                                level: "SYSTEM".to_string(),
                                message: format!("Уровень свободы изменен на: {:?}", new_freedom),
                            });
                            ctx_for_freedom.request_repaint();
                        }

                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    }
                });

                let m_for_cmd = m_for_spawn.clone();
                tokio::spawn(async move {
                    while let Some(cmd) = cmd_rx.recv().await {
                        if cmd == "ROTATE_BARK" {
                            let mut m = m_for_cmd.lock().await;
                            m.rotate_bark().await;
                        }
                    }
                });

                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    
                    let (local_routes, local_id, roots) = {
                        let m_guard = m_for_spawn.lock().await;
                        (m_guard.get_local_routes(), m_guard.local_info.id, m_guard.roots.clone())
                    };
                    
                    let mut discovered = Vec::new();
                    for root in roots {
                        if let Ok(peers) = root.discover(local_routes.clone()).await {
                            discovered.extend(peers);
                        }
                    }
                    
                    let had_changes = !discovered.is_empty();
                    
                    if had_changes {
                        let mut m_guard = m_for_spawn.lock().await;
                        for (info, routes) in discovered {
                            m_guard.routing_table.update_from_neighbor(local_id, info.clone(), &routes);
                            m_guard.known_nodes.insert(info.id, info.clone());
                            
                            if let Some(dtn) = &m_guard.dtn
                                && let Ok(packets) = dtn.take_transit_packets(info.id)
                                    && !packets.is_empty() {
                                        let _ = tx_for_scan.send(LogEvent { level: "DTN".to_string(), message: format!("Извлечено {} пакетов для {}", packets.len(), info.id) });
                                        ctx_for_scan.request_repaint();
                                        for encrypted_payload in packets {
                                            let needles = taiga_resin::split_into_needles(&encrypted_payload, info.id, 200);
                                            for needle in needles {
                                                m_guard.broadcast_needle(info.id, needle.clone()).await;
                                            }
                                        }
                                    }
                        }
                        ctx_for_scan.request_repaint();
                    }

                    // Очистка старых фрагментов в мультиплексоре (Resin GC) и Маршрутов
                    {
                        let mut asm = assembler_clone.lock().await;
                        let removed = asm.clear_abandoned(std::time::Duration::from_secs(60));
                        if removed > 0 {
                            let _ = tx_for_scan.send(LogEvent { level: "SYSTEM".to_string(), message: format!("Resin GC: Удалено {} зависших пакетов", removed) });
                        }
                        
                        let mut m_guard = m_for_spawn.lock().await;
                        let stale_routes = m_guard.routing_table.cleanup_stale_routes(300); // 5 минут TTL
                        if stale_routes > 0 {
                            let _ = tx_for_scan.send(LogEvent { level: "ROUTING".to_string(), message: format!("Очищено {} мертвых маршрутов", stale_routes) });
                        }
                        
                        if let Some(dtn) = &m_guard.dtn {
                            if let Ok(removed_dtn) = dtn.cleanup_expired() {
                                if removed_dtn > 0 {
                                    let _ = tx_for_scan.send(LogEvent { level: "DTN".to_string(), message: format!("Очищено {} протухших пакетов", removed_dtn) });
                                }
                            }
                        }
                    }
                }
            });
        });

        Self {
            mycelium: mycelium_ref,
            logs: Vec::new(),
            local_info: None,
            rx,
            routes: Vec::new(),
            proxy_enabled: false,
            current_tab: Tab::Dashboard,
            cmd_tx,
        }
    }
}

impl eframe::App for TaigaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(egui::Visuals::dark());

        while let Ok(msg) = self.rx.try_recv() {
            self.logs.push(msg);
            if self.logs.len() > 200 {
                self.logs.remove(0); // Ограничиваем лог
            }
        }
        
        if let Ok(m) = self.mycelium.try_lock() {
            self.local_info = Some(m.local_info.clone());
            self.routes.clear();
            for (target_id, (route, _)) in &m.routing_table.entries {
                let next_hop = route.path.first().cloned().unwrap_or(Uuid::nil());
                let hops = route.path.len() as u32;
                let freedom = route.target_info.freedom;
                self.routes.push((*target_id, next_hop, hops, freedom));
            }
        }

        #[allow(unused_mut)]
        let mut header_frame = egui::Frame::side_top_panel(&ctx.style());
        #[cfg(target_os = "android")]
        {
            // Отступ под "челку" (notch) и статус-бар на Android (около 35 пикселей)
            header_frame.inner_margin.top = 35;
        }

        egui::TopBottomPanel::top("header").frame(header_frame).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("TAIGA 🌲 Router Dashboard");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("🔄 Сбросить Кору").clicked() {
                        let _ = self.cmd_tx.send("ROTATE_BARK".to_string());
                        self.logs.push(LogEvent { level: "SYSTEM".to_string(), message: "Кора сброшена! Новый ID сгенерирован.".to_string() });
                    }
                });
            });
            ui.separator();
            if let Some(info) = &self.local_info {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Node ID: {}", info.id)).monospace().color(egui::Color32::LIGHT_GREEN));
                    ui.separator();
                    ui.label(format!("Роль: {:?}", info.status));
                    ui.separator();
                    ui.label(format!("Уровень Свободы: {:?}", info.freedom));
                });
            }
        });

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_tab, Tab::Dashboard, "📡 Маршруты");
                ui.selectable_value(&mut self.current_tab, Tab::Logs, "📜 Журнал");
                ui.selectable_value(&mut self.current_tab, Tab::Settings, "⚙ Настройки");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.current_tab {
                Tab::Dashboard => {
                    ui.heading("Интерфейсы");
                    ui.label("🟢 UDP Симуляция (Локально)");
                    ui.label("🔴 BLE Scanner / GATT Server");
                    ui.label("🔴 Wi-Fi Direct (P2P)");
                    
                    ui.separator();
                    
                    ui.heading("Таблица Маршрутов");
                    egui::ScrollArea::vertical().id_salt("routes_scroll").show(ui, |ui| {
                        egui::Grid::new("routing_grid").striped(true).show(ui, |ui| {
                            ui.label("Target");
                            ui.label("Next Hop");
                            ui.label("Hops");
                            ui.label("Свобода");
                            ui.end_row();

                            for (target, next_hop, hops, freedom) in &self.routes {
                                ui.label(egui::RichText::new(&target.to_string()[0..8]).monospace());
                                ui.label(egui::RichText::new(&next_hop.to_string()[0..8]).monospace());
                                ui.label(hops.to_string());
                                let freedom_text = match freedom {
                                    taiga_mycelium::FreedomLevel::None => "🚫 Локально",
                                    taiga_mycelium::FreedomLevel::WhitelistOnly => "🏛 Белые Списки",
                                    taiga_mycelium::FreedomLevel::Normal => "🌍 Вне Списков",
                                    taiga_mycelium::FreedomLevel::Full => "🚀 Полный (VPN)",
                                };
                                ui.label(freedom_text);
                                ui.end_row();
                            }
                        });
                    });
                }
                Tab::Logs => {
                    ui.heading("Системный Журнал Узла");
                    ui.separator();
                    
                    egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                        for log in &self.logs {
                            let color = match log.level.as_str() {
                                "SYSTEM" => egui::Color32::LIGHT_BLUE,
                                "NETWORK" => egui::Color32::YELLOW,
                                "ROUTING" => egui::Color32::LIGHT_GREEN,
                                "PROXY" => egui::Color32::KHAKI,
                                "DELIVERY" => egui::Color32::WHITE,
                                "DTN" => egui::Color32::LIGHT_RED,
                                "GOSSIP" => egui::Color32::GOLD,
                                "WIFI" => egui::Color32::CYAN,
                                _ => egui::Color32::GRAY,
                            };
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("[{}]", log.level)).color(color).strong());
                                ui.label(&log.message);
                            });
                        }
                    });
                }
                Tab::Settings => {
                    ui.heading("Локальный SOCKS5");
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut self.proxy_enabled, "SOCKS5 Прокси на 127.0.0.1:1080").changed() {
                            self.logs.push(LogEvent {
                                level: "PROXY".to_string(),
                                message: if self.proxy_enabled { "Прокси включен. Маршрутизация трафика в Mesh-сеть..." } else { "Прокси выключен." }.to_string()
                            });
                        }
                    });
                }
            }
        });
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: android_activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info),
    );

    let data_dir = app.internal_data_path().unwrap_or_else(|| std::path::PathBuf::from(".taiga_data"));

    let mut options = eframe::NativeOptions::default();
    options.android_app = Some(app);

    let _ = eframe::run_native(
        "TAIGA",
        options,
        Box::new(move |cc| Ok(Box::new(TaigaApp::new(cc, data_dir)))),
    );
}
