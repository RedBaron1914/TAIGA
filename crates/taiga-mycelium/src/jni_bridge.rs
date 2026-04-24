#![cfg(target_os = "android")]

use jni::JNIEnv;
use jni::objects::{JClass, JString, JByteArray};
use uuid::Uuid;
use std::sync::Mutex;
use std::sync::Arc;
use lazy_static::lazy_static;

#[derive(Debug, Clone)]
pub enum JniEvent {
    BleDeviceDiscovered(String, Vec<u8>),
    BleMessageReceived(String, Vec<u8>),
    WifiDirectConnected { ip: String, is_group_owner: bool },
    WifiDirectDisconnected,
}

lazy_static! {
    pub static ref JNI_EVENT_TX: Arc<Mutex<Option<std::sync::mpsc::Sender<JniEvent>>>> = Arc::new(Mutex::new(None));
    pub static ref ANDROID_NODE_ID: Arc<Mutex<Option<Uuid>>> = Arc::new(Mutex::new(None));
    pub static ref ANDROID_JVM: Arc<Mutex<Option<jni::JavaVM>>> = Arc::new(Mutex::new(None));
}

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _res: *mut std::ffi::c_void) -> jni::sys::jint {
    let mut lock = ANDROID_JVM.lock().unwrap();
    *lock = Some(vm);
    jni::sys::JNI_VERSION_1_6
}

pub fn send_ble_message_to_kotlin(mac: &str, payload: &[u8]) {
    if let Some(vm) = ANDROID_JVM.lock().unwrap().as_ref() {
        if let Ok(mut env) = vm.attach_current_thread() {
            if let Ok(mac_jstring) = env.new_string(mac) {
                if let Ok(payload_jbytearray) = env.byte_array_from_slice(payload) {
                    let _ = env.call_static_method(
                        "com/taiga/mesh/MyceliumCore",
                        "sendBleMessage",
                        "(Ljava/lang/String;[B)V",
                        &[jni::objects::JValue::from(&mac_jstring), jni::objects::JValue::from(&payload_jbytearray)],
                    );
                }
            }
        }
    }
}

pub fn get_android_node_id() -> Option<Uuid> {
    *ANDROID_NODE_ID.lock().unwrap()
}

pub fn has_physical_internet() -> bool {
    if let Some(vm) = ANDROID_JVM.lock().unwrap().as_ref() {
        if let Ok(mut env) = vm.attach_current_thread() {
            if let Ok(result) = env.call_static_method(
                "com/taiga/mesh/MyceliumCore",
                "hasPhysicalInternet",
                "()Z",
                &[],
            ) {
                return result.z().unwrap_or(false);
            }
        }
    }
    false
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_taiga_mesh_MyceliumCore_initNodeId<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    node_id_bytes: JByteArray<'local>,
) {
    let bytes = env.convert_byte_array(&node_id_bytes).expect("Invalid byte array");
    if bytes.len() == 16 {
        if let Ok(id) = Uuid::from_slice(&bytes) {
            let mut lock = ANDROID_NODE_ID.lock().unwrap();
            *lock = Some(id);
            log::info!("[JNI] Инициализирован Node ID из Android: {}", id);
        }
    }
}

pub fn set_jni_sender(tx: std::sync::mpsc::Sender<JniEvent>) {
    let mut lock = JNI_EVENT_TX.lock().unwrap();
    *lock = Some(tx);
}

fn send_event(event: JniEvent) {
    if let Some(tx) = JNI_EVENT_TX.lock().unwrap().as_ref() {
        let _ = tx.send(event);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_taiga_mesh_MyceliumCore_onBleDeviceDiscovered<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    mac_address: JString<'local>,
    node_id_bytes: JByteArray<'local>,
) {
    let mac: String = env.get_string(&mac_address).expect("Invalid MAC string").into();
    let bytes = env.convert_byte_array(&node_id_bytes).expect("Invalid byte array");

    if bytes.len() == 16 {
        if let Ok(id) = Uuid::from_slice(&bytes) {
            log::info!("[JNI] Найдено Дерево по BLE: MAC={}, ID={}", mac, id);
            send_event(JniEvent::BleDeviceDiscovered(mac, bytes));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_taiga_mesh_MyceliumCore_onBleMessageReceived<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    sender_mac: JString<'local>,
    payload: JByteArray<'local>,
) {
    let mac: String = env.get_string(&sender_mac).expect("Invalid MAC string").into();
    let data = env.convert_byte_array(&payload).expect("Invalid byte array");
    
    log::info!("[JNI] Получены байты ({} шт.) по BLE от {}", data.len(), mac);
    send_event(JniEvent::BleMessageReceived(mac, data));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_taiga_mesh_MyceliumCore_onWifiDirectConnected<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    ip_address: JString<'local>,
    is_group_owner: jni::sys::jboolean,
) {
    let ip: String = env.get_string(&ip_address).expect("Invalid IP string").into();
    let is_go = is_group_owner != 0;
    
    log::info!("[JNI] Wi-Fi Direct Connected! IP: {}, GroupOwner: {}", ip, is_go);
    send_event(JniEvent::WifiDirectConnected { ip, is_group_owner: is_go });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_taiga_mesh_MyceliumCore_onWifiDirectDisconnected<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    log::info!("[JNI] Wi-Fi Direct Disconnected!");
    send_event(JniEvent::WifiDirectDisconnected);
}
