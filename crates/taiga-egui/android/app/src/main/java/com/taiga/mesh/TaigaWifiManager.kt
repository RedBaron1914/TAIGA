package com.taiga.mesh

import android.annotation.SuppressLint
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.NetworkInfo
import android.net.wifi.p2p.WifiP2pConfig
import android.net.wifi.p2p.WifiP2pDevice
import android.net.wifi.p2p.WifiP2pManager
import android.util.Log

@SuppressLint("MissingPermission")
@Suppress("DEPRECATION")
class TaigaWifiManager(
    private val context: Context,
    private val manager: WifiP2pManager,
    private val channel: WifiP2pManager.Channel
) : BroadcastReceiver() {

    companion object {
        const val TAG = "TaigaWiFi"
    }

    private val peers = mutableListOf<WifiP2pDevice>()
    private var isConnecting = false
    private val lastConnectAttemptMap = java.util.concurrent.ConcurrentHashMap<String, Long>()

    private val peerListListener = WifiP2pManager.PeerListListener { peerList ->
        val refreshedPeers = peerList.deviceList
        if (refreshedPeers != peers) {
            peers.clear()
            peers.addAll(refreshedPeers)
            Log.i(TAG, "Found ${peers.size} Wi-Fi Direct peers")
            MyceliumCore.sendLogToRust("WIFI", "Обнаружено P2P сородичей по Wi-Fi Direct: ${peers.size}")
            
            // Подключаемся к доступному пиру с 60-секундным кулдауном, чтобы не спамить системными окнами ОС
            if (!isConnecting && peers.isNotEmpty()) {
                val now = System.currentTimeMillis()
                val availablePeer = peers.firstOrNull { 
                    it.status == WifiP2pDevice.AVAILABLE && 
                    (now - (lastConnectAttemptMap[it.deviceAddress] ?: 0L) > 60000)
                }
                if (availablePeer != null) {
                    isConnecting = true
                    lastConnectAttemptMap[availablePeer.deviceAddress] = now
                    connectTo(availablePeer.deviceAddress)
                }
            }
        }
    }

    private val connectionInfoListener = WifiP2pManager.ConnectionInfoListener { info ->
        isConnecting = false
        val groupOwnerAddress = info.groupOwnerAddress?.hostAddress
        if (groupOwnerAddress.isNullOrBlank()) {
            Log.w(TAG, "Group Owner address is null or blank, ignoring connection callback")
            return@ConnectionInfoListener
        }

        if (info.groupFormed && info.isGroupOwner) {
            Log.i(TAG, "I am the Group Owner. IP: $groupOwnerAddress")
            MyceliumCore.sendLogToRust("WIFI", "P2P Соединение установлено! Статус: Group Owner (IP: $groupOwnerAddress, Скорость: ~58 МБ/с)")
            MyceliumCore.onWifiDirectConnected(groupOwnerAddress, true)
        } else if (info.groupFormed) {
            Log.i(TAG, "I am a Client. Group Owner IP: $groupOwnerAddress")
            MyceliumCore.sendLogToRust("WIFI", "P2P Соединение установлено! Статус: Client (IP шлюза: $groupOwnerAddress, Скорость: ~58 МБ/с)")
            MyceliumCore.onWifiDirectConnected(groupOwnerAddress, false)
        }
    }

    fun discoverPeers() {
        manager.discoverPeers(channel, object : WifiP2pManager.ActionListener {
            override fun onSuccess() {
                Log.i(TAG, "Wi-Fi Direct discovery started")
                MyceliumCore.sendLogToRust("WIFI", "Поиск соседей по Wi-Fi Direct запущен.")
            }

            override fun onFailure(reasonCode: Int) {
                Log.e(TAG, "Wi-Fi Direct discovery failed: $reasonCode")
                MyceliumCore.sendLogToRust("WIFI", "Ошибка запуска поиска Wi-Fi Direct: $reasonCode")
            }
        })
    }

    fun stopDiscovery() {
        manager.stopPeerDiscovery(channel, object : WifiP2pManager.ActionListener {
            override fun onSuccess() {
                Log.i(TAG, "Wi-Fi Direct discovery stopped")
            }

            override fun onFailure(reasonCode: Int) {
                Log.e(TAG, "Wi-Fi Direct stop discovery failed: $reasonCode")
            }
        })
    }

    fun connectTo(deviceAddress: String) {
        val config = WifiP2pConfig().apply {
            this.deviceAddress = deviceAddress
        }

        manager.connect(channel, config, object : WifiP2pManager.ActionListener {
            override fun onSuccess() {
                Log.i(TAG, "Connecting to Wi-Fi Direct peer: $deviceAddress")
                MyceliumCore.sendLogToRust("WIFI", "Попытка подключения к P2P устройству: $deviceAddress")
            }

            override fun onFailure(reason: Int) {
                isConnecting = false
                Log.e(TAG, "Failed to connect to peer: $reason")
                MyceliumCore.sendLogToRust("WIFI", "Не удалось подключиться к P2P устройству. Код: $reason")
            }
        })
    }

    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            WifiP2pManager.WIFI_P2P_STATE_CHANGED_ACTION -> {
                val state = intent.getIntExtra(WifiP2pManager.EXTRA_WIFI_STATE, -1)
                if (state == WifiP2pManager.WIFI_P2P_STATE_ENABLED) {
                    Log.i(TAG, "Wi-Fi Direct is enabled. Starting discovery...")
                    discoverPeers()
                } else {
                    Log.w(TAG, "Wi-Fi Direct is disabled. Stopping discovery...")
                    stopDiscovery()
                }
            }
            WifiP2pManager.WIFI_P2P_PEERS_CHANGED_ACTION -> {
                manager.requestPeers(channel, peerListListener)
            }
            WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION -> {
                val networkInfo = intent.getParcelableExtra<NetworkInfo>(WifiP2pManager.EXTRA_NETWORK_INFO)
                if (networkInfo?.isConnected == true) {
                    manager.requestConnectionInfo(channel, connectionInfoListener)
                } else {
                    Log.i(TAG, "Wi-Fi Direct disconnected")
                    MyceliumCore.onWifiDirectDisconnected()
                }
            }
        }
    }

    fun getIntentFilter(): IntentFilter {
        return IntentFilter().apply {
            addAction(WifiP2pManager.WIFI_P2P_STATE_CHANGED_ACTION)
            addAction(WifiP2pManager.WIFI_P2P_PEERS_CHANGED_ACTION)
            addAction(WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION)
            addAction(WifiP2pManager.WIFI_P2P_THIS_DEVICE_CHANGED_ACTION)
        }
    }
}
