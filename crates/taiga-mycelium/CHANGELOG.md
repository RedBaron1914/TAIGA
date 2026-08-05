# Changelog

## [1.1.0](https://github.com/RedBaron1914/TAIGA/compare/taiga-mycelium-v1.0.1...taiga-mycelium-v1.1.0) (2026-04-28)


### Features

* Add is_virtual_uplink flag and routing penalty for symbiotic VPN nodes ([9a2f97d](https://github.com/RedBaron1914/TAIGA/commit/9a2f97d7ebc4b1399baea2acec8b2728921ad8d2))
* **ui:** Bridge backend and Android core events to the Egui UI Journal ([c83ec84](https://github.com/RedBaron1914/TAIGA/commit/c83ec847b677763131a85f068637419da7657ad1))


### Bug Fixes

* Add ConnectivityManager safeguard to prevent fake Full freedom nodes ([4c61140](https://github.com/RedBaron1914/TAIGA/commit/4c611409202ab86788a02e6fd87f733ffd887a6c))
* Eliminate JNI Mutex deadlocks and remove duplicate MeshPayload enum ([014c1fa](https://github.com/RedBaron1914/TAIGA/commit/014c1fa03df6b93f0f04dd393686c58fa87476ce))
* Remove JNI_OnLoad definition which caused eframe/android-activity startup crashes on Android ([9cc6720](https://github.com/RedBaron1914/TAIGA/commit/9cc67200bc69fa2cee1418bc5f9d1f2aab4997b5))
* Resolve critical production bugs (stream ID collisions, multi-hop lookups, JNI deadlocks, and stale route aging) ([745898b](https://github.com/RedBaron1914/TAIGA/commit/745898b437971578f25281389611b154aefe48be))
* Resolve UI hangs by removing Mycelium lock across await boundaries, and prevent RoutingTable overwrite bugs ([25cba9a](https://github.com/RedBaron1914/TAIGA/commit/25cba9a45ee95efc5864b145e6b3bc5364922e32))
* **routing:** Propagate local_info updates to Roots so freedom level broadcasts correctly ([4462e1e](https://github.com/RedBaron1914/TAIGA/commit/4462e1ef9a5015f4fc5586d7291dffb4258305c7))
