package com.taiga.mesh

import android.annotation.SuppressLint
import android.bluetooth.*
import android.bluetooth.le.*
import android.content.Context
import android.os.ParcelUuid
import android.util.Log
import java.util.UUID

@SuppressLint("MissingPermission") // В реальном проекте права запрашиваются через Activity/egui
@Suppress("DEPRECATION")
class TaigaBleManager(private val context: Context, private var localNodeId: ByteArray) {
    companion object {
        const val TAG = "TaigaBLE"
        
        // Те же UUID, что мы определили в Rust (lazy_static)
        val SERVICE_UUID: UUID = UUID.fromString("7A16A000-0000-4000-8000-000000000000")
        val RX_CHAR_UUID: UUID = UUID.fromString("7A16A001-0000-4000-8000-000000000000")
    }

    private val bluetoothManager = context.getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
    private val bluetoothAdapter: BluetoothAdapter? = bluetoothManager.adapter
    private var gattServer: BluetoothGattServer? = null
    
    private var scanCallback: ScanCallback? = null
    private var advertiseCallback: AdvertiseCallback? = null

    private val bleScanner: BluetoothLeScanner?
        get() = bluetoothAdapter?.bluetoothLeScanner
        
    private val bleAdvertiser: BluetoothLeAdvertiser?
        get() = bluetoothAdapter?.bluetoothLeAdvertiser
    private val activeGatts = java.util.concurrent.ConcurrentHashMap<String, BluetoothGatt>()
    private val outgoingQueues = java.util.concurrent.ConcurrentHashMap<String, java.util.ArrayDeque<ByteArray>>()
    private val isWritingMap = java.util.concurrent.ConcurrentHashMap<String, Boolean>()

    fun start() {
        if (bluetoothAdapter == null || !bluetoothAdapter.isEnabled) {
            Log.e(TAG, "Bluetooth is not enabled or not supported.")
            return
        }
        
        startGattServer()
        startAdvertising()
        startScanning()
    }

    fun stop() {
        try {
            scanHandler.removeCallbacks(scanRestartRunnable)
            scanCallback?.let { bleScanner?.stopScan(it) }
            advertiseCallback?.let { bleAdvertiser?.stopAdvertising(it) }
            gattServer?.close()
            gattServer = null

            for ((mac, gatt) in activeGatts) {
                try {
                    gatt.disconnect()
                    gatt.close()
                } catch (e: Exception) {
                    Log.e(TAG, "Error closing GATT for $mac: ${e.message}")
                }
            }
            activeGatts.clear()
            outgoingQueues.clear()
            isWritingMap.clear()

            Log.i(TAG, "BLE Manager stopped cleanly.")
        } catch (e: Exception) {
            Log.e(TAG, "Error stopping BLE: ${e.message}")
        }
    }

    fun updateNodeId(newNodeId: ByteArray) {
        localNodeId = newNodeId
        restartAdvertising()
    }

    fun restartAdvertising() {
        try {
            advertiseCallback?.let { bleAdvertiser?.stopAdvertising(it) }
            startAdvertising()
            Log.i(TAG, "BLE Advertising restarted with updated Manufacturer Data.")
        } catch (e: Exception) {
            Log.e(TAG, "Error restarting BLE Advertising: ${e.message}")
        }
    }

    private fun startGattServer() {
        val serverCallback = object : BluetoothGattServerCallback() {
            override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
                Log.i(TAG, "GATT Server Connection State Change: $newState from ${device.address}")
            }

            override fun onCharacteristicWriteRequest(
                device: BluetoothDevice,
                requestId: Int,
                characteristic: BluetoothGattCharacteristic,
                preparedWrite: Boolean,
                responseNeeded: Boolean,
                offset: Int,
                value: ByteArray
            ) {
                super.onCharacteristicWriteRequest(device, requestId, characteristic, preparedWrite, responseNeeded, offset, value)
                
                if (characteristic.uuid == RX_CHAR_UUID) {
                    Log.i(TAG, "Received ${value.size} bytes from ${device.address}")
                    // Передаем полученные байты (Хвоинку) в Rust-ядро через JNI
                    MyceliumCore.onBleMessageReceived(device.address, value)
                    
                    if (responseNeeded) {
                        gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
                    }
                }
            }
        }

        gattServer = bluetoothManager.openGattServer(context, serverCallback)
        
        val service = BluetoothGattService(SERVICE_UUID, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        val rxCharacteristic = BluetoothGattCharacteristic(
            RX_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_WRITE or BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE,
            BluetoothGattCharacteristic.PERMISSION_WRITE
        )
        
        service.addCharacteristic(rxCharacteristic)
        gattServer?.addService(service)
        
        Log.i(TAG, "GATT Server started")
    }

    private fun startAdvertising() {
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setConnectable(true)
            .setTimeout(0)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
            .build()

        // Основной пакет: только Service UUID (чтобы влезть в лимит 31 байт)
        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .addServiceUuid(ParcelUuid(SERVICE_UUID))
            .build()

        // Scan Response: Manufacturer Data с нашим NodeID (16 байт) + Freedom Level (1 байт) + isVirtualUplink (1 байт)
        val prefs = context.getSharedPreferences("taiga_prefs", Context.MODE_PRIVATE)
        val freedomLevel = prefs.getInt("freedom_level", 0).toByte()
        val isVirtualUplink = if (prefs.getBoolean("is_virtual_uplink", false)) 1.toByte() else 0.toByte()

        val manufacturerData = ByteArray(22)
        System.arraycopy(localNodeId, 0, manufacturerData, 0, 16)
        manufacturerData[16] = freedomLevel
        manufacturerData[17] = isVirtualUplink

        // Криптографическая привязка (4 байта SHA-256 от Node ID) для защиты от MAC Spoofing
        try {
            val pubkeyHash = java.security.MessageDigest.getInstance("SHA-256").digest(localNodeId)
            System.arraycopy(pubkeyHash, 0, manufacturerData, 18, 4)
        } catch (e: Exception) {
            Log.e(TAG, "Error generating pubkey hash", e)
        }

        val scanResponse = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .addManufacturerData(0x1337, manufacturerData) 
            .build()

        advertiseCallback = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
                Log.i(TAG, "Advertising started successfully")
                MyceliumCore.sendLogToRust("NETWORK", "BLE GATT Сервер и Реклама запущены.")
            }

            override fun onStartFailure(errorCode: Int) {
                Log.e(TAG, "Advertising failed with error: $errorCode")
                MyceliumCore.sendLogToRust("NETWORK", "Ошибка запуска BLE рекламы: $errorCode. Возможно устройство не поддерживает BLE Peripheral.")
            }
        }

        bleAdvertiser?.startAdvertising(settings, data, scanResponse, advertiseCallback)
    }

    private val scanHandler = android.os.Handler(android.os.Looper.getMainLooper())
    private val scanRestartRunnable = object : Runnable {
        override fun run() {
            try {
                scanCallback?.let { bleScanner?.stopScan(it) }
                scanCallback?.let { bleScanner?.startScan(listOf(ScanFilter.Builder().setServiceUuid(ParcelUuid(SERVICE_UUID)).build()), ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build(), it) }
            } catch (e: Exception) {
                Log.e(TAG, "Error recycling BLE scanner: ${e.message}")
            }
            scanHandler.postDelayed(this, 30000)
        }
    }

    private val lastDiscoveredNeighbors = java.util.concurrent.ConcurrentHashMap<String, Pair<ByteArray, Long>>()

    private fun startScanning() {
        val filter = ScanFilter.Builder()
            .setServiceUuid(ParcelUuid(SERVICE_UUID))
            .build()

        val settings = ScanSettings.Builder()
            .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
            .build()

        scanCallback = object : ScanCallback() {
            override fun onScanResult(callbackType: Int, result: ScanResult) {
                val manufacturerData = result.scanRecord?.getManufacturerSpecificData(0x1337)
                if (manufacturerData != null) {
                    val mac = result.device.address
                    val now = System.currentTimeMillis()
                    val lastRecord = lastDiscoveredNeighbors[mac]

                    // Вызываем JNI только если это новое устройство, изменились данные или прошло больше 5000 мс
                    if (lastRecord == null || !lastRecord.first.contentEquals(manufacturerData) || (now - lastRecord.second > 5000)) {
                        lastDiscoveredNeighbors[mac] = Pair(manufacturerData.clone(), now)
                        MyceliumCore.onBleDeviceDiscovered(mac, manufacturerData)
                    }
                }
            }

            override fun onScanFailed(errorCode: Int) {
                Log.e(TAG, "Scan failed with error: $errorCode")
                MyceliumCore.sendLogToRust("NETWORK", "Ошибка BLE сканирования: $errorCode")
            }
        }

        bleScanner?.startScan(listOf(filter), settings, scanCallback)
        scanHandler.removeCallbacks(scanRestartRunnable)
        scanHandler.postDelayed(scanRestartRunnable, 30000)
        
        Log.i(TAG, "BLE Scanner started")
        MyceliumCore.sendLogToRust("NETWORK", "BLE Сканер эфира запущен.")
    }

    fun sendMessage(macAddress: String, payload: ByteArray) {
        val queue = outgoingQueues.getOrPut(macAddress) { java.util.ArrayDeque() }
        synchronized(queue) {
            queue.add(payload)
        }

        val existingGatt = activeGatts[macAddress]
        if (existingGatt != null) {
            sendNextInQueue(macAddress, existingGatt)
        } else {
            val device = bluetoothAdapter?.getRemoteDevice(macAddress) ?: return
            Log.i(TAG, "Connecting to $macAddress (queue size: ${queue.size})")

            device.connectGatt(context, false, object : BluetoothGattCallback() {
                override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
                    if (newState == BluetoothProfile.STATE_CONNECTED) {
                        Log.i(TAG, "Connected to $macAddress, requesting MTU...")
                        activeGatts[macAddress] = gatt
                        gatt.requestMtu(256)
                    } else if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                        Log.i(TAG, "Disconnected from $macAddress, closing GATT...")
                        activeGatts.remove(macAddress)
                        outgoingQueues.remove(macAddress)
                        isWritingMap.remove(macAddress)
                        gatt.close()
                    }
                }

                override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
                    Log.i(TAG, "MTU changed to $mtu for $macAddress, discovering services...")
                    gatt.discoverServices()
                }

                override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
                    sendNextInQueue(macAddress, gatt)
                }

                override fun onCharacteristicWrite(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, status: Int) {
                    isWritingMap[macAddress] = false
                    val remaining = outgoingQueues[macAddress]?.size ?: 0
                    Log.i(TAG, "GATT write to $macAddress finished with status $status, remaining in queue: $remaining")
                    if (remaining > 0) {
                        sendNextInQueue(macAddress, gatt)
                    }
                }
            })
        }
    }

    private fun sendNextInQueue(macAddress: String, gatt: BluetoothGatt) {
        if (isWritingMap[macAddress] == true) {
            return
        }

        val queue = outgoingQueues[macAddress] ?: return
        val payload = synchronized(queue) { queue.poll() } ?: return

        val service = gatt.getService(SERVICE_UUID)
        val characteristic = service?.getCharacteristic(RX_CHAR_UUID)
        if (characteristic != null) {
            isWritingMap[macAddress] = true
            characteristic.value = payload
            characteristic.writeType = BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
            val success = gatt.writeCharacteristic(characteristic)
            Log.i(TAG, "GATT write started to $macAddress: $success")
            if (!success) {
                isWritingMap[macAddress] = false
            }
        } else {
            Log.e(TAG, "Taiga service or characteristic not found on $macAddress")
            activeGatts.remove(macAddress)
            outgoingQueues.remove(macAddress)
            isWritingMap.remove(macAddress)
            gatt.disconnect()
        }
    }
}
