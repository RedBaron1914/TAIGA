package com.taiga.mesh

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.BroadcastReceiver
import android.os.Build
import android.os.IBinder
import android.util.Log
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothManager
import android.net.wifi.p2p.WifiP2pManager

class TaigaService : Service() {
    companion object {
        const val TAG = "TaigaService"
        const val NOTIFICATION_ID = 1337
        const val CHANNEL_ID = "taiga_mesh_channel"
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
                        Log.i(TAG, "Bluetooth выключен пользователем. Останавливаем BLE.")
                        bleManager?.stop()
                        bleManager = null
                    }
                    BluetoothAdapter.STATE_ON -> {
                        Log.i(TAG, "Bluetooth включен. Запускаем BLE.")
                        startBleTransport()
                    }
                }
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        Log.i(TAG, "Creating TaigaService")
        MyceliumCore.service = this
        
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, createNotification())

        registerReceiver(bluetoothStateReceiver, IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED))

        startBleTransport()
        startWifiTransport()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Keep service running
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? {
        return null
    }

    override fun onDestroy() {
        super.onDestroy()
        Log.i(TAG, "Destroying TaigaService")
        
        try {
            unregisterReceiver(bluetoothStateReceiver)
        } catch (e: Exception) {
            // Ignore
        }

        bleManager?.stop()
        bleManager = null

        wifiManager?.let {
            try {
                unregisterReceiver(it)
            } catch (e: Exception) {
                // Ignore if not registered
            }
            it.stopDiscovery()
        }
        wifiManager = null

        if (MyceliumCore.service === this) {
            MyceliumCore.service = null
        }
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val name = "TAIGA Mesh Network"
            val descriptionText = "Ensures background connectivity for the TAIGA mesh network"
            val importance = NotificationManager.IMPORTANCE_LOW
            val channel = NotificationChannel(CHANNEL_ID, name, importance).apply {
                description = descriptionText
            }
            val notificationManager: NotificationManager =
                getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            notificationManager.createNotificationChannel(channel)
        }
    }

    private fun createNotification(): Notification {
        val pendingIntent: PendingIntent = Intent(this, MainActivity::class.java).let { notificationIntent ->
            val flags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            } else {
                PendingIntent.FLAG_UPDATE_CURRENT
            }
            PendingIntent.getActivity(this, 0, notificationIntent, flags)
        }

        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL_ID)
        } else {
            Notification.Builder(this)
        }

        return builder
            .setContentTitle("TAIGA Mesh Active")
            .setContentText("Maintaining peer connections in the background.")
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setContentIntent(pendingIntent)
            .build()
    }

    private fun startBleTransport() {
        val bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        val bluetoothAdapter = bluetoothManager.adapter
        if (bluetoothAdapter == null || !bluetoothAdapter.isEnabled) {
            Log.w(TAG, "Cannot start BLE: Bluetooth is disabled or not supported.")
            return
        }

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
                wifiManager?.discoverPeers()
            } else {
                Log.e(TAG, "Wi-Fi Direct is not supported on this device.")
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

    fun updateLocalNodeId(uuidStr: String) {
        val uuid = java.util.UUID.fromString(uuidStr)
        val bb = java.nio.ByteBuffer.wrap(ByteArray(16))
        bb.putLong(uuid.mostSignificantBits)
        bb.putLong(uuid.leastSignificantBits)
        bleManager?.updateNodeId(bb.array())
    }

    fun restartBleAdvertising() {
        bleManager?.restartAdvertising()
    }

    fun sendBleMessage(macAddress: String, payload: ByteArray) {
        bleManager?.sendMessage(macAddress, payload)
    }
}
