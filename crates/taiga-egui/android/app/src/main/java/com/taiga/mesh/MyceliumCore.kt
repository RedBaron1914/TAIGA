package com.taiga.mesh

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.util.Log

object MyceliumCore {
    const val TAG = "TaigaJNI"
    
    private var activityRef: java.lang.ref.WeakReference<MainActivity>? = null

    var activity: MainActivity?
        get() = activityRef?.get()
        set(value) {
            activityRef = if (value != null) java.lang.ref.WeakReference(value) else null
        }

    private var serviceRef: java.lang.ref.WeakReference<TaigaService>? = null

    var service: TaigaService?
        get() = serviceRef?.get()
        set(value) {
            serviceRef = if (value != null) java.lang.ref.WeakReference(value) else null
        }

    private var lastBleRestartTime: Long = 0L

    @JvmStatic
    fun pingBypassingVpn(urlString: String): Boolean {
        val ctx = activity ?: service ?: return false
        val cm = ctx.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager ?: return false
        val activeNetwork = cm.activeNetwork ?: return false
        val caps = cm.getNetworkCapabilities(activeNetwork) ?: return false

        if (caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
            !caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) {
            try {
                val url = java.net.URL(urlString)
                val conn = activeNetwork.openConnection(url) as java.net.HttpURLConnection
                conn.connectTimeout = 3000
                conn.readTimeout = 3000
                conn.requestMethod = "GET"
                conn.connect()
                val code = conn.responseCode
                return code in 200..399
            } catch (e: Exception) {
                Log.w(TAG, "Ping failed via physical network for $urlString", e)
            }
        }
        return false
    }

    @JvmStatic
    fun hasPhysicalInternet(): Boolean {
        val ctx = activity ?: service ?: return false
        val cm = ctx.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager ?: return false
        val activeNetwork = cm.activeNetwork ?: return false
        val caps = cm.getNetworkCapabilities(activeNetwork) ?: return false

        // Проверяем через ConnectivityManager (основной путь)
        val cmResult = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
               !caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)
        if (cmResult) return true

        // Fallback: прямой TCP-пинг на DNS-сервер Google (не зависит от системных флагов)
        return try {
            val socket = java.net.Socket()
            socket.connect(java.net.InetSocketAddress("8.8.8.8", 53), 2000)
            socket.close()
            true
        } catch (e: Exception) {
            false
        }
    }

    @JvmStatic
    fun saveFreedomLevel(level: Int, isVirtual: Boolean) {
        val ctx = activity ?: service ?: return
        val prefs = ctx.getSharedPreferences("taiga_prefs", Context.MODE_PRIVATE)
        val oldLevel = prefs.getInt("freedom_level", -1)
        val oldVirtual = prefs.getBoolean("is_virtual_uplink", false)

        prefs.edit()
            .putInt("freedom_level", level)
            .putBoolean("is_virtual_uplink", isVirtual)
            .apply()
        
        Log.i(TAG, "Freedom level saved to SharedPreferences: $level (Virtual: $isVirtual)")

        if (oldLevel != level || oldVirtual != isVirtual) {
            val now = System.currentTimeMillis()
            if (now - lastBleRestartTime > 2000) {
                lastBleRestartTime = now
                service?.restartBleAdvertising()
            }
        }
    }

    @JvmStatic
    fun getSavedFreedomLevel(): Int {
        val ctx = activity ?: service ?: return 0
        return ctx.getSharedPreferences("taiga_prefs", Context.MODE_PRIVATE)
            .getInt("freedom_level", 0)
    }

    @JvmStatic
    fun getSavedVirtualUplink(): Boolean {
        val ctx = activity ?: service ?: return false
        return ctx.getSharedPreferences("taiga_prefs", Context.MODE_PRIVATE)
            .getBoolean("is_virtual_uplink", false)
    }

    @JvmStatic
    fun saveNodeId(uuidStr: String) {
        val ctx = activity ?: service ?: return
        val prefs = ctx.getSharedPreferences("taiga_prefs", Context.MODE_PRIVATE)
        prefs.edit()
            .putString("node_id", uuidStr)
            .apply()
        
        Log.i(TAG, "Node ID updated to: $uuidStr")
        service?.updateLocalNodeId(uuidStr)
    }

    // Нативные функции, реализованные в Rust (jni_bridge.rs)
    
    @JvmStatic
    fun sendBleMessage(macAddress: String, payload: ByteArray) {
        service?.sendBleMessage(macAddress, payload)
    }

    @JvmStatic
    fun runBleStressTest(macAddress: String, count: Int) {
        Log.i(TAG, "Starting BLE Stress Test: sending $count packets to $macAddress")
        val dummyPayload = ByteArray(120) { (it % 256).toByte() }
        for (i in 1..count) {
            sendBleMessage(macAddress, dummyPayload)
        }
    }

    // Передача сохраненного Node ID из Android в Rust при старте
    @JvmStatic
    external fun initNodeId(nodeIdBytes: ByteArray, freedomLevel: Int, isVirtualUplink: Boolean)

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

    @JvmStatic
    external fun sendLogToRust(level: String, message: String)

    init {
        try {
            System.loadLibrary("taiga_egui")
            Log.i(TAG, "Rust JNI library loaded successfully")
        } catch (e: UnsatisfiedLinkError) {
            Log.e(TAG, "Failed to load Rust JNI library", e)
        }
    }
}
