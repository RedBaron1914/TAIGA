use eframe::egui;
use std::sync::Arc;
use tokio::sync::Mutex;
use taiga_mycelium::{Mycelium, NodeStatus, TreeInfo, Root, Onion};
use taiga_mycelium::udp_root::UdpRoot;
use taiga_resin::{ResinAssembler, split_into_needles};
use uuid::Uuid;

pub mod proxy;

#[derive(Clone)]
pub struct LogEvent {
    pub level: String,
    pub message: String,
}

pub struct TaigaApp {
    mycelium: Arc<Mutex<Mycelium>>,
    logs: Vec<LogEvent>,
    local_info: Option<TreeInfo>,
    rx: std::sync::mpsc::Receiver<LogEvent>,
    routes: Vec<(Uuid, Uuid, u32, taiga_mycelium::FreedomLevel)>, // Target, NextHop, Hops, Freedom
    proxy_enabled: bool,
}

impl TaigaApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let (tx, rx) = std::sync::mpsc::channel();
        let port: u16 = (rand::random::<u16>() % 21) + 40000; 

        let id = Uuid::new_v4();
        let mut m = Mycelium::new(id, NodeStatus::Tree);
        
        let app_data_dir = std::path::PathBuf::from(".taiga_data");
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

                #[cfg(target_os = "android")]
                {
                    let (sync_tx, sync_rx) = std::sync::mpsc::channel();
                    taiga_mycelium::jni_bridge::set_jni_sender(sync_tx);
                    
                    let m_for_jni = m_for_spawn.clone();
                    let tx_for_jni = tx.clone();
                    let ctx_for_jni = ctx.clone();
                    
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
                                            m_for_jni.lock().await.attach_root(Box::new(wifi_root));
                                        }
                                    }
                                    _ => {}
                                }
                            } else {
                                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            }
                        }
                    });
                }

                if let Ok(udp_root) = UdpRoot::new(port, local_info).await {
                    let rx_root = udp_root.clone();
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
                    
                    let exit_streams = Arc::new(Mutex::new(std::collections::HashMap::new()));
                    
                    tokio::spawn(async move {
                        let mut seen_needles = std::collections::HashSet::new();
                        loop {
                            if let Ok((sender_id, needle)) = rx_root.receive_needle().await {
                                let needle_id = (needle.cone_id, needle.sequence_number);
                                if !seen_needles.insert(needle_id) { continue; }
                                if seen_needles.len() > 10000 { seen_needles.clear(); }

                                let m_lock = m_for_rx.lock().await;
                                let local_id = m_lock.local_info.id;
                                
                                if needle.target_tree == Uuid::nil() {
                                    if let Some(root) = m_lock.roots.first() {
                                        let _ = root.send_needle(Uuid::nil(), needle.clone()).await;
                                    }
                                    drop(m_lock);
                                    
                                    let mut asm = assembler_clone.lock().await;
                                    if let Some(full_payload) = asm.receive_needle(needle) {
                                        if let Ok(text) = String::from_utf8(full_payload) {
                                            log_rx("GOSSIP", &format!("Шёпот от {}: {}", sender_id.to_string().chars().take(8).collect::<String>(), text));
                                        }
                                    }
                                } else if needle.target_tree == local_id {
                                    drop(m_lock);
                                    let mut asm = assembler_clone.lock().await;
                                    if let Some(full_encrypted_payload) = asm.receive_needle(needle) {
                                        let decrypted = {
                                            let m = m_for_rx.lock().await;
                                            m.crypto.decrypt(&full_encrypted_payload)
                                        };

                                        match decrypted {
                                            Ok(decrypted_bytes) => {
                                                if let Ok(onion) = serde_json::from_slice::<Onion>(&decrypted_bytes) {
                                                    match onion {
                                                        Onion::Core { sender, payload } => {
                                                            if let Ok(mesh_payload) = serde_json::from_slice::<taiga_mycelium::MeshProxyPayload>(&payload) {
                                                                let m_ref_exit = m_for_rx.clone();
                                                                let exit_streams_ref = exit_streams.clone();
                                                                let tx_log_exit = tx_for_rx.clone();
                                                                tokio::spawn(async move {
                                                                    proxy::handle_exit_node_request(mesh_payload, sender, m_ref_exit, exit_streams_ref, tx_log_exit).await;
                                                                });
                                                            } else if let Ok(text) = String::from_utf8(payload) {
                                                                log_rx("DELIVERY", &format!("Доставлен пакет от {}", sender));
                                                            }
                                                        },
                                                        Onion::Layer { next_hop, encrypted_data } => {
                                                            let m_guard = m_for_rx.lock().await;
                                                            if let Some(path) = m_guard.routing_table.get_path(&next_hop) {
                                                                let actual_next_hop = path[0];
                                                                log_rx("ROUTING", &format!("Снят слой луковицы. Пересылка к {}", actual_next_hop));
                                                                let needles = taiga_resin::split_into_needles(&encrypted_data, actual_next_hop, 200);
                                                                if let Some(root) = m_guard.roots.first() {
                                                                    for n in needles {
                                                                        let _ = root.send_needle(actual_next_hop, n).await;
                                                                    }
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
                                            },
                                            Err(_) => {}
                                        }
                                    }
                                } else {
                                    // Пакет не нам, игнорируем
                                }
                            }
                        }
                    });

                    let mut m_guard = m_for_spawn.lock().await;
                    m_guard.attach_root(Box::new(udp_root));
                }
                
                // Запуск локального SOCKS5-прокси сервера
                let local_streams = Arc::new(Mutex::new(std::collections::HashMap::new()));
                let m_ref_proxy = m_for_spawn.clone();
                let tx_for_proxy = tx.clone();
                tokio::spawn(async move {
                    proxy::run_socks5_server(1080, m_ref_proxy, local_streams, tx_for_proxy).await;
                });

                // Фоновый цикл сканирования
                let tx_for_scan = tx.clone();
                let ctx_for_scan = ctx.clone();
                let m_for_freedom = m_for_spawn.clone();
                let tx_for_freedom = tx.clone();
                let ctx_for_freedom = ctx.clone();

                // Фоновый процесс для определения "Уровня Свободы" (FreedomLevel)
                tokio::spawn(async move {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(3))
                        .build()
                        .unwrap();

                    loop {
                        let mut new_freedom = taiga_mycelium::FreedomLevel::None;
                        
                        // 1. Проверяем "Белые списки" (гос. ресурсы, крупные поисковики)
                        let has_whitelist = client.get("https://ya.ru").send().await.is_ok();
                        
                        if has_whitelist {
                            new_freedom = taiga_mycelium::FreedomLevel::WhitelistOnly;
                            
                            // 2. Проверяем выход за пределы "белых списков" (обычные сайты)
                            let has_normal = client.get("https://coworking.tyuiu.ru").send().await.is_ok();
                            if has_normal {
                                new_freedom = taiga_mycelium::FreedomLevel::Normal;
                                
                                // 3. Проверяем полный доступ (VPN/Антизапрет)
                                let has_full = client.get("https://discord.com").send().await.is_ok();
                                if has_full {
                                    new_freedom = taiga_mycelium::FreedomLevel::Full;
                                }
                            }
                        }

                        let mut changed = false;
                        {
                            let mut m_guard = m_for_freedom.lock().await;
                            if m_guard.local_info.freedom != new_freedom {
                                m_guard.local_info.freedom = new_freedom;
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

                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let mut m_guard = m_for_spawn.lock().await;
                    let local_routes = m_guard.get_local_routes();
                    let local_id = m_guard.local_info.id;
                    
                    let mut discovered = Vec::new();
                    for root in &m_guard.roots {
                        if let Ok(peers) = root.discover(local_routes.clone()).await {
                            discovered.extend(peers);
                        }
                    }
                    
                    let had_changes = !discovered.is_empty();
                    for (info, routes) in discovered {
                        m_guard.routing_table.update_from_neighbor(local_id, info.clone(), &routes);
                        m_guard.known_nodes.insert(info.id, info.clone());
                        
                        if let Some(dtn) = &m_guard.dtn {
                            if let Ok(packets) = dtn.take_transit_packets(info.id) {
                                if !packets.is_empty() {
                                    let _ = tx_for_scan.send(LogEvent { level: "DTN".to_string(), message: format!("Извлечено {} пакетов для {}", packets.len(), info.id) });
                                    ctx_for_scan.request_repaint();
                                    if let Some(root) = m_guard.roots.first() {
                                        for encrypted_payload in packets {
                                            let needles = taiga_resin::split_into_needles(&encrypted_payload, info.id, 200);
                                            for needle in needles {
                                                let _ = root.send_needle(info.id, needle.clone()).await;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if had_changes {
                        ctx_for_scan.request_repaint();
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
            for (target_id, route) in &m.routing_table.entries {
                let next_hop = route.path.first().cloned().unwrap_or(Uuid::nil());
                let hops = route.path.len() as u32;
                let freedom = route.target_info.freedom;
                self.routes.push((*target_id, next_hop, hops, freedom));
            }
        }

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("TAIGA 🌲 Router Dashboard");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("🔄 Сбросить Кору").clicked() {
                        let m_ref = self.mycelium.clone();
                        std::thread::spawn(move || {
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async move {
                                let mut m = m_ref.lock().await;
                                m.rotate_bark().await;
                            });
                        });
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

        egui::SidePanel::left("left_panel").min_width(250.0).show(ctx, |ui| {
            ui.heading("Интерфейсы");
            ui.label("🟢 UDP Симуляция (Локально)");
            ui.label("🔴 BLE Scanner / GATT Server");
            ui.label("🔴 Wi-Fi Direct (P2P)");
            
            ui.separator();
            
            ui.heading("Локальный SOCKS5");
            ui.horizontal(|ui| {
                if ui.checkbox(&mut self.proxy_enabled, "SOCKS5 Прокси на 127.0.0.1:1080").changed() {
                    self.logs.push(LogEvent {
                        level: "PROXY".to_string(),
                        message: if self.proxy_enabled { "Прокси включен. Маршрутизация трафика в Mesh-сеть..." } else { "Прокси выключен." }.to_string()
                    });
                }
            });

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
        });

        egui::CentralPanel::default().show(ctx, |ui| {
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

    let mut options = eframe::NativeOptions::default();
    options.android_app = Some(app);

    let _ = eframe::run_native(
        "TAIGA",
        options,
        Box::new(|cc| Ok(Box::new(TaigaApp::new(cc)))),
    );
}
