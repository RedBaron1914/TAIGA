# Changelog

## [1.0.2](https://github.com/RedBaron1914/TAIGA/compare/taiga-egui-v1.0.1...taiga-egui-v1.1.0) (2026-04-28)


### Features

* Add custom cyber-mycelium app icon ([af297e5](https://github.com/RedBaron1914/TAIGA/commit/af297e5171f13856e44f8d9ac56cbefa3fd95fbe))
* Add is_virtual_uplink flag and routing penalty for symbiotic VPN nodes ([9a2f97d](https://github.com/RedBaron1914/TAIGA/commit/9a2f97d7ebc4b1399baea2acec8b2728921ad8d2))
* **ui:** Bridge backend and Android core events to the Egui UI Journal ([c83ec84](https://github.com/RedBaron1914/TAIGA/commit/c83ec847b677763131a85f068637419da7657ad1))


### Bug Fixes

* Add ACCESS_NETWORK_STATE permission to fix ConnectivityManager crash ([43d8460](https://github.com/RedBaron1914/TAIGA/commit/43d8460394f204fc1ed07b9f5c466e792435a808))
* Add ConnectivityManager safeguard to prevent fake Full freedom nodes ([4c61140](https://github.com/RedBaron1914/TAIGA/commit/4c611409202ab86788a02e6fd87f733ffd887a6c))
* Add top margin for Android status bar / notch safe zone ([168565d](https://github.com/RedBaron1914/TAIGA/commit/168565d512f6f42b048a4317f4159b5db735c42c))
* **android:** Dynamically handle Wi-Fi Direct state changes ([3c6e627](https://github.com/RedBaron1914/TAIGA/commit/3c6e6279f6bba1592a31c8cc50896f5ca160beec))
* **android:** Implement graceful BLE state handling and Bluetooth permission fallback ([c752ecd](https://github.com/RedBaron1914/TAIGA/commit/c752ecd94347cba7ddbadcc12e3cf797927e3435))
* Detect virtual internet using active local SOCKS5 connections to prevent Hairpinning over Mesh VPNs ([e160357](https://github.com/RedBaron1914/TAIGA/commit/e16035712af328d2040dfb27b4b5cb0975a91bc8))
* Prefix unused client variable in bg ping task ([45062eb](https://github.com/RedBaron1914/TAIGA/commit/45062eb3acf55e534e9027b891844e66b751f098))
* Resolve BLE advertising size limit and enable UI logging for discovery ([45279d2](https://github.com/RedBaron1914/TAIGA/commit/45279d27b38e42378c1eba4dcf6518a24d5b72de))
* Resolve critical production bugs (stream ID collisions, multi-hop lookups, JNI deadlocks, and stale route aging) ([745898b](https://github.com/RedBaron1914/TAIGA/commit/745898b437971578f25281389611b154aefe48be))
* Resolve HashMap type inference errors and UI compilation bugs ([dacb39e](https://github.com/RedBaron1914/TAIGA/commit/dacb39ecc54b6f4e0c341fc80e2716038c8a0935))
* Resolve UI hangs by removing Mycelium lock across await boundaries, and prevent RoutingTable overwrite bugs ([25cba9a](https://github.com/RedBaron1914/TAIGA/commit/25cba9a45ee95efc5864b145e6b3bc5364922e32))
* Revert ping_bypassing_vpn to reqwest pings to enable VPN tunneling over Mesh ([8b2a540](https://github.com/RedBaron1914/TAIGA/commit/8b2a540b8e5e1e89712911844b1a9630cf2b5fc9))
* **routing:** Bypass strict TLS verification for background freedom level checks on Android ([87f55e0](https://github.com/RedBaron1914/TAIGA/commit/87f55e06ea593a4b489e018f26521c6112260336))
* **routing:** Propagate local_info updates to Roots so freedom level broadcasts correctly ([4462e1e](https://github.com/RedBaron1914/TAIGA/commit/4462e1ef9a5015f4fc5586d7291dffb4258305c7))
* **ui:** Wrap system log messages on mobile screens ([934d8c6](https://github.com/RedBaron1914/TAIGA/commit/934d8c64541b5ab264644849ce4619adf5b75bd0))
