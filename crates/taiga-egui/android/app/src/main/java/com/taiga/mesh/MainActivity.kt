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
import android.content.Intent
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothManager
import android.content.BroadcastReceiver

class MainActivity : GameActivity() {
    companion object {
        init {
            // Эта строка заставляет Dalvik загрузить нашу Rust библиотеку (.so)
            System.loadLibrary("taiga_egui")
        }
    }

    private var bleManager: TaigaBleManager? = null
    private var wifiManager: TaigaWifiManager? = null

    private val bluetoothStateReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            val action = intent.action
            if (action == BluetoothAdapter.ACTION_STATE_CHANGED) {
                val state = intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)
                when (state) {
                    BluetoothAdapter.STATE_OFF -> {
                        Log.i("TAIGA", "Bluetooth выключен пользователем. Останавливаем BLE.")
                        bleManager?.stop()
                        bleManager = null
                    }
                    BluetoothAdapter.STATE_ON -> {
                        Log.i("TAIGA", "Bluetooth включен. Запускаем BLE.")
                        startBleTransport()
                    }
                }
            }
        }
    }

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

        // Регистрируем слушатель для отслеживания состояния Bluetooth
        registerReceiver(bluetoothStateReceiver, IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED))

        // Начиная с Android 6+ (и особенно 12+) разрешения нужно запрашивать "вживую"
        requestTaigaPermissions()
    }

    override fun onDestroy() {
        super.onDestroy()
        try {
            unregisterReceiver(bluetoothStateReceiver)
        } catch (e: Exception) {
            Log.e("TAIGA", "Error unregistering receiver", e)
        }
        bleManager?.stop()
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
            MyceliumCore.sendLogToRust("SYSTEM", "Запрашиваем разрешения у пользователя: Location, BLE, Wi-Fi...")
            requestPermissions(missingPermissions, 1337)
        } else {
            Log.i("TAIGA", "Все разрешения уже выданы.")
            MyceliumCore.sendLogToRust("SYSTEM", "Все разрешения выданы, запускаем транспорты...")
            checkBluetoothAndStart()
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == 1337) {
            val allGranted = grantResults.all { it == PackageManager.PERMISSION_GRANTED }
            if (allGranted) {
                Log.i("TAIGA", "Пользователь дал добро! Проверяем Bluetooth.")
                MyceliumCore.sendLogToRust("SYSTEM", "Пользователь дал разрешения! Запускаем сеть.")
                checkBluetoothAndStart()
            } else {
                Log.e("TAIGA", "Пользователь отказал в разрешениях. Сеть работать не будет!")
                MyceliumCore.sendLogToRust("SYSTEM", "ОТКАЗ В ПРАВАХ: Bluetooth и Wi-Fi P2P работать не будут!")
                // При отказе в разрешениях мы больше не вызываем startTransports()
            }
        }
    }

    private fun checkBluetoothAndStart() {
        val bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        val bluetoothAdapter = bluetoothManager.adapter
        
        if (bluetoothAdapter == null) {
            Log.e("TAIGA", "Bluetooth is not supported on this device.")
            MyceliumCore.sendLogToRust("NETWORK", "Bluetooth аппаратно не поддерживается на этом устройстве.")
            startWifiTransport()
        } else if (!bluetoothAdapter.isEnabled) {
            Log.i("TAIGA", "Bluetooth выключен. Запрашиваем включение у пользователя...")
            MyceliumCore.sendLogToRust("NETWORK", "Bluetooth выключен. Ждем включения от пользователя...")
            val enableBtIntent = Intent(BluetoothAdapter.ACTION_REQUEST_ENABLE)
            startActivityForResult(enableBtIntent, 1338)
            // Wi-Fi можем запустить параллельно, не дожидаясь ответа по Bluetooth
            startWifiTransport()
        } else {
            startBleTransport()
            startWifiTransport()
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == 1338) {
            if (resultCode == android.app.Activity.RESULT_OK) {
                Log.i("TAIGA", "Пользователь включил Bluetooth.")
                MyceliumCore.sendLogToRust("NETWORK", "Bluetooth успешно включен пользователем!")
                startBleTransport()
            } else {
                Log.e("TAIGA", "Пользователь отказался включать Bluetooth.")
                MyceliumCore.sendLogToRust("NETWORK", "Пользователь отклонил запрос на включение Bluetooth. Работаем только по Wi-Fi.")
            }
        }
    }

    private fun startBleTransport() {
        if (bleManager == null) {
            val nodeId = getOrCreateNodeId()
            bleManager = TaigaBleManager(this, nodeId)
            bleManager?.start()
        }
    }

    private fun startWifiTransport() {
        if (wifiManager == null) {
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

