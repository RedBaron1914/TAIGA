package com.taiga.mesh

import android.util.Log

object MyceliumCore {
    const val TAG = "TaigaJNI"

    // Нативные функции, реализованные в Rust (jni_bridge.rs)
    
    // Вызывается сканером, когда найдено устройство с сервисом Тайги
    @JvmStatic
    external fun onBleDeviceDiscovered(macAddress: String, nodeIdBytes: ByteArray)

    // Вызывается GATT-сервером, когда кто-то прислал нам "Хвоинку"
    @JvmStatic
    external fun onBleMessageReceived(senderMac: String, payload: ByteArray)

    @JvmStatic
    external fun onWifiDirectConnected(groupOwnerIp: String, isGroupOwner: Boolean)

    @JvmStatic
    external fun onWifiDirectDisconnected()

    init {
        try {
            // Имя динамической библиотеки, которую скомпилирует cargo-apk / xbuild
            // Если наш бинарник называется taiga-egui, библиотека будет libtaiga_egui.so
            System.loadLibrary("taiga_egui")
            Log.i(TAG, "Rust JNI library loaded successfully")
        } catch (e: UnsatisfiedLinkError) {
            Log.e(TAG, "Failed to load Rust JNI library", e)
        }
    }
}
