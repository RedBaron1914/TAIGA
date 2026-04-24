package com.taiga.mesh

import android.os.Bundle
import android.os.Build
import android.Manifest
import android.content.pm.PackageManager
import com.google.androidgamesdk.GameActivity
import android.util.Log
import android.content.Context
import android.net.wifi.p2p.WifiP2pManager
import android.content.IntentFilter

class MainActivity : GameActivity() {
    companion object {
        init {
            // Эта строка заставляет Dalvik загрузить нашу Rust библиотеку (.so)
            System.loadLibrary("taiga_egui")
        }
    }

    private var bleManager: TaigaBleManager? = null
    private var wifiManager: TaigaWifiManager? = null

    private fun getOrCreateNodeId(): ByteArray {
        val prefs = getSharedPreferences("taiga_prefs", Context.MODE_PRIVATE)
        var idString = prefs.getString("node_id", null)
        if (idString == null) {
            idString = java.util.UUID.randomUUID().toString()
            prefs.edit().putString("node_id", idString).apply()
        }
        val uuid = java.util.UUID.fromString(idString)
        val bb = java.nio.ByteBuffer.wrap(ByteArray(16))
        bb.putLong(uuid.mostSignificantBits)
        bb.putLong(uuid.leastSignificantBits)
        return bb.array()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        // Регистрируем синглтон для JNI
        MyceliumCore.activity = this
        
        // Генерируем или читаем сохраненный уникальный Node ID и передаем в Rust ДО запуска GameActivity
        val nodeId = getOrCreateNodeId()
        MyceliumCore.initNodeId(nodeId)

        super.onCreate(savedInstanceState)
        
        Log.i("TAIGA", "Starting GameActivity for egui...")

        // Начиная с Android 6+ (и особенно 12+) разрешения нужно запрашивать "вживую"
        requestTaigaPermissions()
    }

    private fun requestTaigaPermissions() {
        val permissions = mutableListOf(
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION
        )

        // На Android 12+ (API 31) добавились новые жесткие права для Bluetooth
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            permissions.add(Manifest.permission.BLUETOOTH_SCAN)
            permissions.add(Manifest.permission.BLUETOOTH_CONNECT)
            permissions.add(Manifest.permission.BLUETOOTH_ADVERTISE)
        }
        
        // Android 13+ права для Wi-Fi Direct
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            permissions.add(Manifest.permission.NEARBY_WIFI_DEVICES)
        }

        val missingPermissions = permissions.filter {
            checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED
        }.toTypedArray()

        if (missingPermissions.isNotEmpty()) {
            Log.i("TAIGA", "Запрашиваем разрешения у пользователя...")
            requestPermissions(missingPermissions, 1337)
        } else {
            Log.i("TAIGA", "Все разрешения уже выданы.")
            startTransports()
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == 1337) {
            val allGranted = grantResults.all { it == PackageManager.PERMISSION_GRANTED }
            if (allGranted) {
                Log.i("TAIGA", "Пользователь дал добро! Запускаем BLE и Wi-Fi Direct.")
                startTransports()
            } else {
                Log.e("TAIGA", "Пользователь отказал в разрешениях. Сеть работать не будет!")
                startTransports() 
            }
        }
    }

    private fun startTransports() {
        val nodeId = getOrCreateNodeId()
        
        // Запуск BLE-менеджера
        bleManager = TaigaBleManager(this, nodeId)
        bleManager?.start()
        
        // Запуск Wi-Fi Direct менеджера
        val p2pManager = getSystemService(Context.WIFI_P2P_SERVICE) as WifiP2pManager?
        if (p2pManager != null) {
            val channel = p2pManager.initialize(this, mainLooper, null)
            wifiManager = TaigaWifiManager(this, p2pManager, channel)
            registerReceiver(wifiManager, wifiManager?.getIntentFilter())
            
            // Запускаем поиск пиров по Wi-Fi Direct
            wifiManager?.discoverPeers()
        } else {
            Log.e("TAIGA", "Wi-Fi Direct is not supported on this device.")
        }
    }

    fun sendBleMessage(macAddress: String, payload: ByteArray) {
        bleManager?.sendMessage(macAddress, payload)
    }

    override fun onResume() {
        super.onResume()
        wifiManager?.let {
            registerReceiver(it, it.getIntentFilter())
        }
    }

    override fun onPause() {
        super.onPause()
        wifiManager?.let {
            try {
                unregisterReceiver(it)
            } catch (e: IllegalArgumentException) {
                // Игнорируем, если ресивер не был зарегистрирован
            }
        }
    }
}

