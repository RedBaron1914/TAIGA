package com.taiga.mesh

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.util.Log

object MyceliumCore {
    const val TAG = "TaigaJNI"
    
    // Ссылка на активити для обратных вызовов
    var activity: MainActivity? = null

    @JvmStatic
    fun pingBypassingVpn(urlString: String): Boolean {
        val act = activity ?: return false
        val cm = act.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val networks = cm.allNetworks
        for (n in networks) {
            val caps = cm.getNetworkCapabilities(n)
            if (caps != null && 
                caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
                !caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) {
                
                try {
                    val url = java.net.URL(urlString)
                    val conn = n.openConnection(url) as java.net.HttpURLConnection
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
        }
        return false
    }

    @JvmStatic
    fun hasPhysicalInternet(): Boolean {
        val act = activity ?: return false
        val cm = act.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val networks = cm.allNetworks
        for (n in networks) {
            val caps = cm.getNetworkCapabilities(n)
            if (caps != null && 
                caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
                caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) &&
                !caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) {
                return true
            }
        }
        return false
    }

    // Нативные функции, реализованные в Rust (jni_bridge.rs)
    
    @JvmStatic
    fun sendBleMessage(macAddress: String, payload: ByteArray) {
        activity?.sendBleMessage(macAddress, payload)
    }

    // Передача сохраненного Node ID из Android в Rust при старте
    @JvmStatic
    external fun initNodeId(nodeIdBytes: ByteArray)

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
            System.loadLibrary("taiga_egui")
            Log.i(TAG, "Rust JNI library loaded successfully")
        } catch (e: UnsatisfiedLinkError) {
            Log.e(TAG, "Failed to load Rust JNI library", e)
        }
    }
}
