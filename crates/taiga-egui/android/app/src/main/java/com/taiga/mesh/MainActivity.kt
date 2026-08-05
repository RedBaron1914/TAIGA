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

    // Вся логика BLE и Wi-Fi Direct перенесена в фоновую службу TaigaService

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
        val prefs2 = getSharedPreferences("taiga_prefs", Context.MODE_PRIVATE)
        val freedomLevel = prefs2.getInt("freedom_level", 0)
        val isVirtualUplink = prefs2.getBoolean("is_virtual_uplink", false)
        MyceliumCore.initNodeId(nodeId, freedomLevel, isVirtualUplink)

        super.onCreate(savedInstanceState)
        
        Log.i("TAIGA", "Starting GameActivity for egui...")

        // Начиная с Android 6+ (и особенно 12+) разрешения нужно запрашивать "вживую"
        requestTaigaPermissions()
    }

    override fun onDestroy() {
        super.onDestroy()
        if (MyceliumCore.activity === this) {
            MyceliumCore.activity = null
        }
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
        
        // Android 13+ права для Wi-Fi Direct и уведомлений (для Foreground Service)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            permissions.add(Manifest.permission.NEARBY_WIFI_DEVICES)
            permissions.add(Manifest.permission.POST_NOTIFICATIONS)
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

    private fun startTaigaService() {
        Log.i("TAIGA", "Запускаем TaigaService...")
        val intent = Intent(this, TaigaService::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
    }

    private fun checkBluetoothAndStart() {
        val bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        val bluetoothAdapter = bluetoothManager.adapter
        
        if (bluetoothAdapter == null) {
            Log.e("TAIGA", "Bluetooth is not supported on this device.")
            MyceliumCore.sendLogToRust("NETWORK", "Bluetooth аппаратно не поддерживается на этом устройстве.")
            startTaigaService()
        } else if (!bluetoothAdapter.isEnabled) {
            Log.i("TAIGA", "Bluetooth выключен. Запрашиваем включение у пользователя...")
            MyceliumCore.sendLogToRust("NETWORK", "Bluetooth выключен. Ждем включения от пользователя...")
            val enableBtIntent = Intent(BluetoothAdapter.ACTION_REQUEST_ENABLE)
            startActivityForResult(enableBtIntent, 1338)
            // Запускаем службу в любом случае, BLE поднимется после включения Bluetooth
            startTaigaService()
        } else {
            startTaigaService()
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == 1338) {
            if (resultCode == android.app.Activity.RESULT_OK) {
                Log.i("TAIGA", "Пользователь включил Bluetooth.")
                MyceliumCore.sendLogToRust("NETWORK", "Bluetooth успешно включен пользователем!")
                // Сервис уже запущен, он автоматически подхватит BLE благодаря bluetoothStateReceiver
            } else {
                Log.e("TAIGA", "Пользователь отказался включать Bluetooth.")
                MyceliumCore.sendLogToRust("NETWORK", "Пользователь отклонил запрос на включение Bluetooth. Работаем только по Wi-Fi.")
            }
        }
    }
}

