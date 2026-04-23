# Подготовка TAIGA для Android (BLE Permissions) 🤖

Поскольку мобильные ОС (особенно Android 12+) очень строго относятся к Bluetooth, чтобы `btleplug` смог работать, нам нужно выполнить два шага: выдать приложению манифест-разрешения и запросить их у пользователя (Runtime Permissions).

Так как у вас сейчас на ПК не настроен `NDK_HOME` (из-за чего Tauri не сгенерировал папку `gen/android`), вот полная инструкция, что нужно будет сделать перед первой сборкой APK.

## Шаг 1: Настройка переменных среды (Windows)

Для того чтобы Tauri смог инициализировать Android-проект, убедитесь, что в переменных среды Windows добавлены:
1. `ANDROID_HOME` = `C:\Users\<ВашеИмя>\AppData\Local\Android\Sdk`
2. `NDK_HOME` = `C:\Users\<ВашеИмя>\AppData\Local\Android\Sdk\ndk\<версия_ndk>` (установите NDK через Android Studio -> SDK Manager -> SDK Tools)

После этого запустите в папке `app`:
```bash
pnpm tauri android init
```

## Шаг 2: Изменение AndroidManifest.xml

Когда папка `app/src-tauri/gen/android` появится, откройте файл `app/src-tauri/gen/android/app/src/main/AndroidManifest.xml` и добавьте перед тегом `<application>` следующие строки:

```xml
    <!-- Разрешения для работы с Bluetooth (Android 12+) -->
    <uses-permission android:name="android.permission.BLUETOOTH_SCAN" android:usesPermissionFlags="neverForLocation" />
    <uses-permission android:name="android.permission.BLUETOOTH_CONNECT" />
    <uses-permission android:name="android.permission.BLUETOOTH_ADVERTISE" />

    <!-- Для старых версий Android (до 12) -->
    <uses-permission android:name="android.permission.BLUETOOTH" />
    <uses-permission android:name="android.permission.BLUETOOTH_ADMIN" />
    <uses-permission android:name="android.permission.ACCESS_FINE_LOCATION" />
    <uses-permission android:name="android.permission.ACCESS_COARSE_LOCATION" />
```

## Шаг 3: Runtime-запрос разрешений в Tauri

Начиная с Android 6.0, просто указать права в манифесте недостаточно — нужно показать пользователю всплывающее окно. 
В Tauri v2 это проще всего сделать через вызов нативного Intent. В будущем мы добавим небольшой Java/Kotlin плагин в Tauri-проект, который будет вызываться при старте приложения и запрашивать эти права.

Пока что для тестов на эмуляторе или ПК Android-разрешения нас не блокируют, и мы можем сфокусироваться на логике Mesh-сети!

```