plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.taiga.mesh"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.taiga.mesh"
        minSdk = 26
        targetSdk = 34
        versionCode = 2
        versionName = "1.0.1-beta"
        
        setProperty("archivesBaseName", "TAIGA-v${versionName}")

        ndk {
            abiFilters.addAll(arrayOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64"))
        }
    }

    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "armeabi-v7a", "x86", "x86_64")
            isUniversalApk = true
        }
    }

    signingConfigs {
        create("release") {
            val keystoreFile = file("release.keystore")
            if (keystoreFile.exists()) {
                storeFile = keystoreFile
                storePassword = System.getenv("ORG_GRADLE_PROJECT_KEYSTORE_PASSWORD") ?: "taiga123"
                keyAlias = System.getenv("ORG_GRADLE_PROJECT_KEY_ALIAS") ?: "taiga"
                keyPassword = System.getenv("ORG_GRADLE_PROCESS_KEY_PASSWORD") ?: "taiga123"
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            if (file("release.keystore").exists()) {
                signingConfig = signingConfigs.getByName("release")
            } else {
                signingConfig = signingConfigs.getByName("debug")
            }
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    // Сообщаем Android Studio, где искать скомпилированные .so библиотеки
    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }
}

// Кастомный таск для компиляции Rust через cargo ndk
tasks.register<Exec>("cargoBuildAll") {
    val cargoProjectDir = file("../../") // Директория taiga-egui
    val jniLibsDir = file("src/main/jniLibs")
    
    // Создаем директорию для .so файла
    doFirst {
        jniLibsDir.mkdirs()
    }
    
    workingDir(cargoProjectDir)
    
    // Определяем, собирается ли релиз
    val isRelease = gradle.startParameter.taskNames.any { it.contains("Release", ignoreCase = true) }
    
    if (isRelease) {
        commandLine("cargo", "ndk", "-t", "armeabi-v7a", "-t", "arm64-v8a", "-t", "x86", "-t", "x86_64", "-o", "android/app/src/main/jniLibs", "build", "--release")
    } else {
        commandLine("cargo", "ndk", "-t", "armeabi-v7a", "-t", "arm64-v8a", "-t", "x86", "-t", "x86_64", "-o", "android/app/src/main/jniLibs", "build")
    }
}

// Привязываем наш таск к процессу сборки до того, как Gradle начнет паковать APK
tasks.whenTaskAdded {
    if ((name == "javaPreCompileDebug" || name == "javaPreCompileRelease")) {
        dependsOn("cargoBuildAll")
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("androidx.games:games-activity:3.0.4")
}
