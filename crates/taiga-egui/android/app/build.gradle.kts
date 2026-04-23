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
        versionCode = 1
        versionName = "1.0"
        
        ndk {
            abiFilters.add("arm64-v8a")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
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
tasks.register<Exec>("cargoBuildArm64") {
    val cargoProjectDir = file("../../") // Директория taiga-egui
    val jniLibsDir = file("src/main/jniLibs/arm64-v8a")
    
    // Создаем директорию для .so файла
    doFirst {
        jniLibsDir.mkdirs()
    }
    
    workingDir(cargoProjectDir)
    
    // Запускаем cargo ndk -t arm64-v8a build (добавляем --release для релиза)
    commandLine("cargo", "ndk", "-t", "arm64-v8a", "-o", "android/app/src/main/jniLibs", "build")
}

// Привязываем наш таск к процессу сборки до того, как Gradle начнет паковать APK
tasks.whenTaskAdded {
    if ((name == "javaPreCompileDebug" || name == "javaPreCompileRelease")) {
        dependsOn("cargoBuildArm64")
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("androidx.games:games-activity:3.0.4")
}
