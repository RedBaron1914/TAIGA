# TAIGA (Тайга) 🌲

**TAIGA** is an experimental decentralized P2P Mesh networking protocol and SOCKS5 transport designed to maintain connectivity across asymmetric network degradation, "walled-garden" whitelists, and local internet blackouts using Bluetooth LE and Wi-Fi Direct.

> [!WARNING]
> **EXPERIMENTAL PROTOCOL:** This is a Proof-of-Concept (beta) version. It currently lacks advanced protection against Sybil attacks, packet flooding, and malicious transit nodes. It is intended for research and testing of decentralized routing and E2EE mesh transport.

## Key Features

1. **Reachability-Aware Routing (Tier 0–3):**
   Unlike traditional mesh protocols that only count hops, TAIGA continuously probes the actual quality of each node's external internet uplink and prioritizes routes toward the most capable gateway:
   * **Tier 0 (`None`):** Complete offline isolation (local Mesh only).
   * **Tier 1 (`WhitelistOnly`):** Walled-garden / "White-list" mode — only domestic intranet or basic infrastructure endpoints respond; external routing is dropped.
   * **Tier 2 (`Normal`):** Standard ISP access — general global web is reachable, though specific endpoints or protocols may be throttled or filtered at the provider level.
   * **Tier 3 (`Full`):** Unrestricted connectivity — the node has an active encrypted upstream tunnel (VPN/Overlay) or unfiltered link with full access to global endpoints.

2. **SOCKS5 Over Mesh:**
   TAIGA exposes a local SOCKS5 proxy (`127.0.0.1:1080`). Any application (browsers, messengers, CLI tools) can pipe TCP streams into the mesh without root privileges. TAIGA fragments, encrypts, and routes these streams blindly to the best available Exit Node.

3. **Onion Routing (ECIES):**
   Payloads are multi-layer encrypted using `x25519` and `chacha20poly1305`. Transit nodes only see the next-hop UUID; they cannot inspect the payload or identify the original sender or final destination.

4. **Delay-Tolerant Networking (DTN):**
   Features an ACID-compliant persistent Store-and-Forward buffer powered by `redb`. If a next-hop node drops out of radio range while a packet is in transit, encrypted packets are stored on disk and automatically flushed once the link is restored.

5. **Multi-Transport Aggregation & Interactive Testbed:**
   Simultaneous support for Bluetooth LE (GATT), Wi-Fi Direct (P2P), and local UDP loopback. Includes a built-in interactive 2D topology simulator (`--simshow`) for testing node failures and dynamic route convergence.

## Architecture

* **taiga-mycelium:** Core P2P routing table, ECIES cryptography, JNI bridge for Android hardware, and `redb` DTN storage.
* **taiga-resin:** Stream multiplexer/assembler that slices large encrypted packets ("Cones") into 200-byte fragments ("Needles") for low-MTU transports like BLE.
* **taiga-egui:** Pure Rust UI built with `egui`/`eframe`, featuring the SOCKS5 server, Android `GameActivity` integration, and the interactive network simulator.

## Getting Started

### Desktop (Node Mode & Interactive Simulator)
```bash
# Run standard router node (UDP loopback discovery)
cargo run -p taiga-egui --release

# Run interactive 2D topology & failure simulator
cargo run -p taiga-egui --release -- --simshow
```

### Android
Requires Android NDK and `cargo-ndk`:
```bash
cd crates/taiga-egui/android
./gradlew assembleRelease
```

---

**TAIGA** — экспериментальный децентрализованный P2P Mesh-протокол и SOCKS5-транспорт, предназначенный для сохранения связности в условиях асимметричной деградации сети, сегментации трафика («белых списков») и локальных отключений интернета с использованием Bluetooth LE и Wi-Fi Direct.

> [!WARNING]
> **ЭКСПЕРИМЕНТАЛЬНЫЙ ПРОТОКОЛ:** Данная версия является Proof-of-Concept (бета). В ней пока отсутствуют развитые механизмы защиты от Sybil-атак, флуда и вредоносных транзитных узлов. Проект предназначен для исследования алгоритмов децентрализованной маршрутизации и E2EE-транспорта.

## Основные возможности

1. **Маршрутизация с учётом полноты связности (Tier 0–3):**
   В отличие от классических mesh-протоколов, считающих только количество прыжков, TAIGA в фоновом режиме тестирует реальную связность каждого узла с глобальной сетью и строит маршруты к шлюзу с наилучшим качеством канала (с учётом штрафа за дистанцию):
   * **Tier 0 (`None`):** Полная изоляция (шатдаун, работает только локальная Mesh-сеть).
   * **Tier 1 (`WhitelistOnly`):** Режим «белых списков» (Walled Garden) — отвечают только базовые внутренние/локальные ресурсы (например, `ya.ru`), внешний сегмент сети недоступен.
   * **Tier 2 (`Normal`):** Стандартный канал провайдера — базовый глобальный веб работает, но отдельные внешние ресурсы или протоколы могут быть ограничены фильтрацией или сбоями на магистралях.
   * **Tier 3 (`Full`):** Полный доступ — на узле активен рабочий внешний туннель (VPN/оверлей) или нефильтруемый канал со свободной маршрутизацией до любых глобальных узлов.

2. **SOCKS5 over Mesh:**
   Тайга поднимает локальный SOCKS5-прокси (`127.0.0.1:1080`) без необходимости получения root-прав. Любое приложение (браузер, мессенджер, терминал) может направлять TCP-потоки в Mesh-сеть. Протокол фрагментирует, шифрует и «вслепую» выводит трафик через лучшую доступную Экзит-ноду.

3. **Луковая маршрутизация (ECIES):**
   Данные послойно шифруются с использованием `x25519` и `chacha20poly1305`. Промежуточные ретрансляторы видят только UUID следующего прыжка и не могут прочитать содержимое пакета или узнать конечного отправителя/получателя.

4. **Устойчивость к разрывам (DTN):**
   Персистентный буфер Store-and-Forward на базе транзакционной БД `redb`. Если транзитный узел вышел из зоны радиовидимости во время передачи, зашифрованные пакеты сохраняются на диск и автоматически досылаются при восстановлении линка.

5. **Агрегация транспортов и интерактивный симулятор:**
   Одновременная работа поверх Bluetooth LE (GATT), Wi-Fi Direct (P2P) и локального UDP-контура. Включает интерактивный 2D-симулятор топологии (`--simshow`) для наглядного тестирования аварий узлов, перестроения маршрутов и работы DTN.

## Структура проекта

* **taiga-mycelium:** Ядро P2P-маршрутизации, криптография ECIES, JNI-мост для Android и DTN-хранилище `redb`.
* **taiga-resin:** Протокольный мультиплексор для нарезки больших зашифрованных пакетов («Шишек») на мелкие 200-байтные фрагменты («Хвоинки») под каналы с низким MTU.
* **taiga-egui:** Кроссплатформенный интерфейс на базе `egui`/`eframe`, включающий SOCKS5-сервер, интеграцию с Android `GameActivity` и визуальный симулятор сети.

## Сборка и запуск

### Десктоп (Узел и Симулятор)
```bash
# Запуск стандартного узла (локальное обнаружение по UDP)
cargo run -p taiga-egui --release

# Запуск интерактивного 2D-стенда симуляции топологии
cargo run -p taiga-egui --release -- --simshow
```

### Android
Требуется установленный Android NDK и `cargo-ndk`:
```bash
cd crates/taiga-egui/android
./gradlew assembleRelease
```
