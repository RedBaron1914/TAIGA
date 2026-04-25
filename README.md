# TAIGA (Тайга) 🌲

**TAIGA** is an experimental decentralized P2P Mesh network protocol and proxy transport designed to bypass censorship, white-lists, and internet shutdowns in dense urban environments using Bluetooth LE and Wi-Fi Direct.

> [!WARNING]
> **EXPERIMENTAL PROTOCOL:** This is a Proof-of-Concept (beta) version. It lacks advanced protection against Sybil attacks, packet flooding, and malicious nodes. It is intended for research and testing of decentralized routing and E2EE transport. Do not use for mission-critical or life-threatening communication.

## Key Features

1.  **Freedom-Level Aware Routing:** The network automatically assesses the internet accessibility of each node (None, Whitelist-only, Normal, or Full VPN access). The routing engine prioritizes paths through nodes with higher "Freedom Levels" while accounting for a distance (hop-count) penalty.
2.  **SOCKS5 Over Mesh:** TAIGA provides a local SOCKS5 proxy (default: `127.0.0.1:1080`). Any application (Telegram, Browser, VPN client) can tunnel traffic into the Mesh. TAIGA fragments, encrypts, and routes these streams blindly through the network to the most capable Exit Node.
3.  **Onion & Garlic Routing:** Payloads are multi-layer encrypted using ECIES (`x25519` and `chacha20poly1305`). Transit nodes can only see the next hop ID; they cannot read the payload or determine the ultimate sender/receiver.
4.  **Delay-Tolerant Networking (DTN):** Features a persistent Store-and-Forward buffer powered by `redb`. If a destination node is out of range, encrypted packets are stored on disk and delivered automatically when connectivity is restored.
5.  **Multi-Transport Aggregation:** Simultaneous support for UDP (simulation), Bluetooth LE (GATT), and Wi-Fi Direct. The core aggregates traffic from all active roots for maximum reliability.

## Architecture

*   **taiga-mycelium:** Core P2P routing table, ECIES cryptography, JNI bridge for Android hardware, and DTN storage.
*   **taiga-resin:** Multiplexer/De-multiplexer for fragmenting large binary streams into small needles for low-MTU transports.
*   **taiga-egui:** Pure Rust UI built with `egui` and `eframe`, featuring the SOCKS5 server implementation and Android GameActivity integration.

## Getting Started

### Desktop (Simulation)
Run multiple instances on the same local network to test routing and DTN:
```bash
cargo run -p taiga-egui --release
```

### Android
Requires NDK and `cargo-ndk`.
```bash
cd crates/taiga-egui/android
./gradlew assembleRelease
```

---

**TAIGA** — это экспериментальный децентрализованный P2P Mesh-протокол и прокси-транспорт, предназначенный для обхода цензуры, «белых списков» и шатдаунов интернета в условиях плотной городской застройки с использованием Bluetooth LE и Wi-Fi Direct.

> [!WARNING]
> **ЭКСПЕРИМЕНТАЛЬНЫЙ ПРОТОКОЛ:** Данная версия является Proof-of-Concept (бета). В ней отсутствуют развитые механизмы защиты от Sybil-атак, флуда и вредоносных узлов. Проект предназначен для исследовательских целей и тестирования алгоритмов маршрутизации и E2EE-транспорта. Не используйте для передачи критически важной информации в реальных условиях.

## Основные возможности

1.  **Маршрутизация на основе «Уровней Свободы»:** Сеть автоматически оценивает доступность интернета у каждого узла (от полной изоляции до полного VPN-доступа). Движок маршрутизации отдает приоритет узлам с более высоким уровнем доступа, учитывая штраф за расстояние (количество прыжков).
2.  **SOCKS5 over Mesh:** Тайга поднимает локальный SOCKS5-прокси (`127.0.0.1:1080`). Любое приложение (Telegram, браузер, VPN-клиент) может направлять трафик в Mesh-сеть. Тайга фрагментирует, шифрует и «вслепую» передает эти потоки через сеть к наиболее подходящей Экзит-ноде.
3.  **Луковая маршрутизация (Onion & Garlic):** Данные многократно шифруются с использованием ECIES (`x25519` и `chacha20poly1305`). Транзитные узлы видят только ID следующего прыжка; они не могут прочитать содержимое или определить конечного отправителя/получателя.
4.  **Устойчивость к разрывам (DTN):** Персистентный буфер Store-and-Forward на базе БД `redb`. Если узел-получатель вне зоны доступа, зашифрованные пакеты сохраняются на диске и доставляются автоматически при восстановлении связи.
5.  **Агрегация транспортов (Multihoming):** Одновременная поддержка UDP (симуляция), Bluetooth LE (GATT) и Wi-Fi Direct. Ядро объединяет трафик со всех активных интерфейсов для максимальной надежности.

## Структура проекта

*   **taiga-mycelium:** Ядро P2P-маршрутизации, криптография ECIES, JNI-мост для Android и DTN-хранилище.
*   **taiga-resin:** Мультиплексор для нарезки больших бинарных потоков на мелкие фрагменты («хвою») для передачи по каналам с низким MTU.
*   **taiga-egui:** Пользовательский интерфейс на чистом Rust (`egui`/`eframe`), включающий SOCKS5-сервер и интеграцию с Android GameActivity.

## Как запустить

### Десктоп (Симуляция)
Запустите несколько экземпляров в одной локальной сети для тестирования маршрутизации:
```bash
cargo run -p taiga-egui --release
```

### Android
Требуется установленный NDK и `cargo-ndk`.
```bash
cd crates/taiga-egui/android
./gradlew assembleRelease
```
