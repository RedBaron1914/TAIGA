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
    UiLog(String, String),
}

lazy_static! {
    pub static ref JNI_EVENT_TX: Arc<Mutex<Option<std::sync::mpsc::Sender<JniEvent>>>> = Arc::new(Mutex::new(None));
    pub static ref ANDROID_NODE_ID: Arc<Mutex<Option<Uuid>>> = Arc::new(Mutex::new(None));
    pub static ref ANDROID_JVM: Arc<Mutex<Option<jni::JavaVM>>> = Arc::new(Mutex::new(None));
    pub static ref MYCELIUM_CORE_CLASS: Arc<Mutex<Option<jni::objects::GlobalRef>>> = Arc::new(Mutex::new(None));
    /// Уровень свободы, переданный при старте из Kotlin (до запуска Tokio-рантайма)
    pub static ref ANDROID_FREEDOM_LEVEL: Arc<Mutex<Option<(i32, bool)>>> = Arc::new(Mutex::new(None));
}

/// Получает текущую среду JNI без лишнего переприсоединения потока к JVM
fn with_jni_env<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut JNIEnv, &jni::objects::GlobalRef) -> Option<R>,
{
    let vm_guard = ANDROID_JVM.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(vm) = vm_guard.as_ref() {
        if let Ok(mut env) = vm.get_env() {
            let class_guard = MYCELIUM_CORE_CLASS.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(global_ref) = class_guard.as_ref() {
                return f(&mut env, global_ref);
            }
        } else if let Ok(mut guard) = vm.attach_current_thread() {
            let class_guard = MYCELIUM_CORE_CLASS.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(global_ref) = class_guard.as_ref() {
                return f(&mut guard, global_ref);
            }
        }
    }
    None
}

pub fn send_ui_log(level: &str, msg: &str) {
    send_event(JniEvent::UiLog(level.to_string(), msg.to_string()));
}

pub fn send_ble_message_to_kotlin(mac: &str, payload: &[u8]) {
    with_jni_env(|env, global_ref| {
        if let Ok(mac_jstring) = env.new_string(mac) {
            if let Ok(payload_jbytearray) = env.byte_array_from_slice(payload) {
                let class = <&jni::objects::JClass>::from(global_ref.as_obj());
                let _ = env.call_static_method(
                    class,
                    "sendBleMessage",
                    "(Ljava/lang/String;[B)V",
                    &[jni::objects::JValue::from(&mac_jstring), jni::objects::JValue::from(&payload_jbytearray)],
                );
            }
        }
        Some(())
    });
}

pub fn get_android_node_id() -> Option<Uuid> {
    *ANDROID_NODE_ID.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn ping_bypassing_vpn(url: &str) -> bool {
    with_jni_env(|env, global_ref| {
        if let Ok(url_jstring) = env.new_string(url) {
            let class = <&jni::objects::JClass>::from(global_ref.as_obj());
            if let Ok(result) = env.call_static_method(
                class,
                "pingBypassingVpn",
                "(Ljava/lang/String;)Z",
                &[jni::objects::JValue::from(&url_jstring)],
            ) {
                return Some(result.z().unwrap_or(false));
            }
        }
        Some(false)
    }).unwrap_or(false)
}

pub fn has_physical_internet() -> bool {
    with_jni_env(|env, global_ref| {
        let class = <&jni::objects::JClass>::from(global_ref.as_obj());
        match env.call_static_method(class, "hasPhysicalInternet", "()Z", &[]) {
            Ok(result) => {
                let res = result.z().unwrap_or(false);
                log::info!("[JNI] hasPhysicalInternet = {}", res);
                Some(res)
            }
            Err(e) => {
                log::error!("[JNI] hasPhysicalInternet failed: {:?}", e);
                if env.exception_check().unwrap_or(false) {
                    let _ = env.exception_describe();
                    let _ = env.exception_clear();
                }
                Some(false)
            }
        }
    }).unwrap_or_else(|| {
        log::error!("[JNI] with_jni_env returned None in has_physical_internet");
        false
    })
}

pub fn save_freedom_level(level: i32, is_virtual: bool) {
    with_jni_env(|env, global_ref| {
        let class = <&jni::objects::JClass>::from(global_ref.as_obj());
        let _ = env.call_static_method(
            class,
            "saveFreedomLevel",
            "(IZ)V",
            &[
                jni::objects::JValue::from(level),
                jni::objects::JValue::from(is_virtual as jni::sys::jboolean),
            ],
        );
        Some(())
    });
}

pub fn get_saved_freedom_level() -> (i32, bool) {
    // Читаем напрямую из статики, установленной через initNodeId — это гарантированно работает при старте
    if let Some(saved) = *ANDROID_FREEDOM_LEVEL.lock().unwrap_or_else(|e| e.into_inner()) {
        return saved;
    }
    let mut level = 0;
    let mut is_virtual = false;
    with_jni_env(|env, global_ref| {
        let class = <&jni::objects::JClass>::from(global_ref.as_obj());
        if let Ok(res_level) = env.call_static_method(class, "getSavedFreedomLevel", "()I", &[]) {
            level = res_level.i().unwrap_or(0);
        }
        if let Ok(res_virtual) = env.call_static_method(class, "getSavedVirtualUplink", "()Z", &[]) {
            is_virtual = res_virtual.z().unwrap_or(false);
        }
        Some(())
    });
    (level, is_virtual)
}

pub fn save_node_id(uuid_str: &str) {
    with_jni_env(|env, global_ref| {
        if let Ok(jstr) = env.new_string(uuid_str) {
            let class = <&jni::objects::JClass>::from(global_ref.as_obj());
            let _ = env.call_static_method(
                class,
                "saveNodeId",
                "(Ljava/lang/String;)V",
                &[jni::objects::JValue::from(&jstr)],
            );
        }
        Some(())
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_taiga_mesh_MyceliumCore_initNodeId<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    node_id_bytes: JByteArray<'local>,
    freedom_level: jni::sys::jint,
    is_virtual: jni::sys::jboolean,
) {
    if let Ok(vm) = env.get_java_vm() {
        let mut lock = ANDROID_JVM.lock().unwrap_or_else(|e| e.into_inner());
        *lock = Some(vm);
    }
    
    if let Ok(global_class) = env.new_global_ref(&_class) {
        let mut lock = MYCELIUM_CORE_CLASS.lock().unwrap_or_else(|e| e.into_inner());
        *lock = Some(global_class);
    }

    if let Ok(bytes) = env.convert_byte_array(&node_id_bytes) {
        if bytes.len() == 16 {
            if let Ok(id) = Uuid::from_slice(&bytes) {
                let mut lock = ANDROID_NODE_ID.lock().unwrap_or_else(|e| e.into_inner());
                *lock = Some(id);
                log::info!("[JNI] Инициализирован Node ID из Android: {}", id);
            }
        }
    }
    
    // Сохраняем уровень свободы сразу, чтобы Rust мог его прочитать при старте
    {
        let mut lock = ANDROID_FREEDOM_LEVEL.lock().unwrap_or_else(|e| e.into_inner());
        *lock = Some((freedom_level, is_virtual != 0));
    }
    log::info!("[JNI] Начальный уровень свободы: {} (virtual={})", freedom_level, is_virtual != 0);
}

pub fn set_jni_sender(tx: std::sync::mpsc::Sender<JniEvent>) {
    let mut lock = JNI_EVENT_TX.lock().unwrap_or_else(|e| e.into_inner());
    *lock = Some(tx);
}

fn send_event(event: JniEvent) {
    if let Some(tx) = JNI_EVENT_TX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
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
    if let Ok(mac_str) = env.get_string(&mac_address) {
        let mac: String = mac_str.into();
        if let Ok(bytes) = env.convert_byte_array(&node_id_bytes) {
            if bytes.len() >= 16 {
                if let Ok(id) = Uuid::from_slice(&bytes[0..16]) {
                    log::info!("[JNI] Найдено Дерево по BLE: MAC={}, ID={}", mac, id);
                    send_event(JniEvent::BleDeviceDiscovered(mac, bytes));
                }
            }
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
    if let Ok(mac_str) = env.get_string(&sender_mac) {
        let mac: String = mac_str.into();
        if let Ok(data) = env.convert_byte_array(&payload) {
            log::info!("[JNI] Получены байты ({} шт.) по BLE от {}", data.len(), mac);
            send_event(JniEvent::BleMessageReceived(mac, data));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_taiga_mesh_MyceliumCore_onWifiDirectConnected<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    ip_address: JString<'local>,
    is_group_owner: jni::sys::jboolean,
) {
    if let Ok(ip_str) = env.get_string(&ip_address) {
        let ip: String = ip_str.into();
        let is_go = is_group_owner != 0;
        
        log::info!("[JNI] Wi-Fi Direct Connected! IP: {}, GroupOwner: {}", ip, is_go);
        send_event(JniEvent::WifiDirectConnected { ip, is_group_owner: is_go });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_taiga_mesh_MyceliumCore_onWifiDirectDisconnected<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    log::info!("[JNI] Wi-Fi Direct Disconnected!");
    send_event(JniEvent::WifiDirectDisconnected);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_taiga_mesh_MyceliumCore_sendLogToRust<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    level: JString<'local>,
    message: JString<'local>,
) {
    if let Ok(lvl) = env.get_string(&level) {
        if let Ok(msg) = env.get_string(&message) {
            send_ui_log(&String::from(lvl), &String::from(msg));
        }
    }
}
