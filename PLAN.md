# TAIGA Roadmap 🌲

Project development is divided into stages reflecting the growth of our "forest."

### v0.1.0: "Seedling" (Росток) — Simulation & Core
* [x] Definition of architecture, terminology, and workspace structure (Initial Core).
* [x] Implementation of `UdpRoot` — local UDP simulator for testing.
* [x] Basic UI prototype: node status display (Tree/Clearing) and neighbor list.
* [x] Basic message transfer between simulated nodes.

### v0.2.0: "Resin" (Смола) — Multiplexing
* [x] Development of the `taiga-resin` crate.
* [x] Logic for fragmenting large packets (Cones) into needles.
* [x] Assembly logic and sequence control on the receiving side.
* [x] Support for parallel roots transmission.

### v0.3.0: "Roots" (Корни) — Physical Layer
* [x] Integration of `btleplug` for desktop.
* [x] Native Android BLE GATT Server/Client implementation via Kotlin and JNI.
* [x] Successful Discovery of real devices via Bluetooth.
* [x] Bidirectional BLE bridge between Rust and Kotlin.

### v0.4.0: "Mycelium" (Мицелий) — Mesh Routing
* [x] Implementation of Path Vector routing in `taiga-mycelium`.
* [x] Multihoming: simultaneous traffic aggregation across BLE, WiFi, and UDP.
* [x] DTN (Delay-Tolerant Networking) Store-and-Forward persistent buffer using `redb`.
* [x] Identification of "Clearing" (Exit) nodes based on "Freedom Levels."

### v0.5.0: "Canopy" (Крона) — Global Exit
* [x] Realization of Exit Node logic: proxying Mesh requests to the real internet.
* [x] End-to-End Encryption (E2EE) using ECIES (x25519 + Chacha20Poly1305).
* [x] Onion Routing: transit nodes only see the next hop.
* [x] Smart route selection: prioritizing high "Freedom Levels" with distance penalties.

### v1.0.0: "Taiga" (Тайга) — Production Hardening & Release
* [x] **UI Overhaul:** Fully migrated from Tauri to pure Rust `egui` for performance.
* [x] **Secure Tunneling:** Full bidirectional SOCKS5 proxy server over Mesh.
* [x] **Anti-Hairpinning:** Automatic detection and penalization of virtual (tunneled) uplinks.
* [x] **Stability:** Resolved JNI deadlocks, memory leaks, and storage leaks.
* [x] **Identity:** Persistent Node UUIDs stored in Android SharedPreferences.
* [x] **Android Release:** Signed, optimized release APK with custom cyber-branding.

### v1.1.0+: Future Growth
* [ ] **Bincode Serialization:** Replace JSON with binary format for 40% less radio airtime.
* [ ] **Forward Secrecy:** Ephemeral key exchange per stream session.
* [ ] **Traffic Masking:** Fixed-size packet padding to obscure metadata.
* [ ] **Proof-of-Work (PoW):** Anti-spam protection for route announcements.
* [ ] **Trust Scores:** Decentralized reputation system to isolate malicious nodes.
