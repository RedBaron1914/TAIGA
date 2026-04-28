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
class TaigaBleManager(private val context: Context, private val localNodeId: ByteArray) {
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
            scanCallback?.let { bleScanner?.stopScan(it) }
            advertiseCallback?.let { bleAdvertiser?.stopAdvertising(it) }
            gattServer?.close()
            gattServer = null
            Log.i(TAG, "BLE Manager stopped cleanly.")
        } catch (e: Exception) {
            Log.e(TAG, "Error stopping BLE: ${e.message}")
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

        val manufacturerData = ByteArray(18)
        System.arraycopy(localNodeId, 0, manufacturerData, 0, 16)
        manufacturerData[16] = freedomLevel
        manufacturerData[17] = isVirtualUplink

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
                    // Мы нашли соседа (Дерево)! Отправляем его MAC и ID в Rust
                    MyceliumCore.onBleDeviceDiscovered(result.device.address, manufacturerData)
                }
            }

            override fun onScanFailed(errorCode: Int) {
                Log.e(TAG, "Scan failed with error: $errorCode")
                MyceliumCore.sendLogToRust("NETWORK", "Ошибка BLE сканирования: $errorCode")
            }
        }

        bleScanner?.startScan(listOf(filter), settings, scanCallback)
        
        Log.i(TAG, "BLE Scanner started")
        MyceliumCore.sendLogToRust("NETWORK", "BLE Сканер эфира запущен.")
    }

    fun sendMessage(macAddress: String, payload: ByteArray) {
        val device = bluetoothAdapter?.getRemoteDevice(macAddress) ?: return
        Log.i(TAG, "Connecting to $macAddress to send ${payload.size} bytes")
        
        device.connectGatt(context, false, object : BluetoothGattCallback() {
            override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
                if (newState == BluetoothProfile.STATE_CONNECTED) {
                    Log.i(TAG, "Connected to $macAddress, requesting MTU...")
                    gatt.requestMtu(256)
                } else if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                    gatt.close()
                }
            }

            override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
                Log.i(TAG, "MTU changed to $mtu, discovering services...")
                gatt.discoverServices()
            }

            override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
                val service = gatt.getService(SERVICE_UUID)
                val characteristic = service?.getCharacteristic(RX_CHAR_UUID)
                if (characteristic != null) {
                    characteristic.value = payload
                    characteristic.writeType = BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
                    val success = gatt.writeCharacteristic(characteristic)
                    Log.i(TAG, "GATT write started: $success")
                } else {
                    Log.e(TAG, "Taiga service or characteristic not found on $macAddress")
                    gatt.disconnect()
                }
            }

            override fun onCharacteristicWrite(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, status: Int) {
                Log.i(TAG, "GATT write finished with status $status, disconnecting.")
                gatt.disconnect()
            }
        })
    }
}
