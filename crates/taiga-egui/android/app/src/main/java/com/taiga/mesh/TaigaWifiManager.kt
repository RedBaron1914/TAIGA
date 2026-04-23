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
class TaigaWifiManager(
    private val context: Context,
    private val manager: WifiP2pManager,
    private val channel: WifiP2pManager.Channel
) : BroadcastReceiver() {

    companion object {
        const val TAG = "TaigaWiFi"
    }

    private val peers = mutableListOf<WifiP2pDevice>()

    private val peerListListener = WifiP2pManager.PeerListListener { peerList ->
        val refreshedPeers = peerList.deviceList
        if (refreshedPeers != peers) {
            peers.clear()
            peers.addAll(refreshedPeers)
            Log.i(TAG, "Found ${peers.size} Wi-Fi Direct peers")
        }
    }

    private val connectionInfoListener = WifiP2pManager.ConnectionInfoListener { info ->
        val groupOwnerAddress = info.groupOwnerAddress?.hostAddress
        if (info.groupFormed && info.isGroupOwner) {
            Log.i(TAG, "I am the Group Owner. IP: $groupOwnerAddress")
            MyceliumCore.onWifiDirectConnected(groupOwnerAddress ?: "", true)
        } else if (info.groupFormed) {
            Log.i(TAG, "I am a Client. Group Owner IP: $groupOwnerAddress")
            MyceliumCore.onWifiDirectConnected(groupOwnerAddress ?: "", false)
        }
    }

    fun discoverPeers() {
        manager.discoverPeers(channel, object : WifiP2pManager.ActionListener {
            override fun onSuccess() {
                Log.i(TAG, "Wi-Fi Direct discovery started")
            }

            override fun onFailure(reasonCode: Int) {
                Log.e(TAG, "Wi-Fi Direct discovery failed: $reasonCode")
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
            }

            override fun onFailure(reason: Int) {
                Log.e(TAG, "Failed to connect to peer: $reason")
            }
        })
    }

    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            WifiP2pManager.WIFI_P2P_STATE_CHANGED_ACTION -> {
                val state = intent.getIntExtra(WifiP2pManager.EXTRA_WIFI_STATE, -1)
                if (state == WifiP2pManager.WIFI_P2P_STATE_ENABLED) {
                    Log.i(TAG, "Wi-Fi Direct is enabled")
                } else {
                    Log.w(TAG, "Wi-Fi Direct is not enabled")
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
