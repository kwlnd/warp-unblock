plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "dev.warpunblock"
    compileSdk = 34

    defaultConfig {
        applicationId = "dev.warpunblock"
        minSdk = 24
        targetSdk = 34
        versionCode = (System.getenv("VERSION_CODE") ?: "1").toInt()
        versionName = System.getenv("VERSION_NAME") ?: "0.7.7"
    }

    // Подпись для хобби-проекта: ключ лежит в репозитории, чтобы обновления ставились поверх.
    // Это НЕ ключ для магазина приложений.
    signingConfigs {
        create("hobby") {
            storeFile = file("../hobby-signing.jks")
            storePassword = "warp-unblock"
            keyAlias = "warp-unblock"
            keyPassword = "warp-unblock"
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("hobby")
        }
    }

    // Ядро mihomo запускается как процесс из папки нативных библиотек, поэтому их нужно распаковывать.
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }

    // APK под arm64-v8a и universal (arm64 + armeabi-v7a: ТВ-приставки и 32-битные системы).
    // Отдельный armeabi-v7a тоже собирается, но в релиз не идёт.
    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "armeabi-v7a")
            isUniversalApk = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    testOptions {
        unitTests {
            isIncludeAndroidResources = true
            // Снимки экранов: gradle testDebugUnitTest -Pscreenshots=1 → app/build/screenshots/
            all {
                val shots = (project.findProperty("screenshots") ?: "").toString()
                it.systemProperty("warp.screenshots", shots)
                if (shots.isNotEmpty()) it.systemProperty("roborazzi.test.record", "true")
            }
        }
    }
    buildFeatures {
        compose = true
        buildConfig = true
    }
    // Общие с Windows каталоги: профили WARP и способы регистрации берутся из windows/app/config.
    sourceSets["main"].assets.srcDir(layout.buildDirectory.dir("generated/sharedAssets"))
}

val sharedAssets = tasks.register<Copy>("sharedAssets") {
    from("../../windows/app/config") {
        include("strategies.json", "registration.json")
    }
    into(layout.buildDirectory.dir("generated/sharedAssets"))
}
tasks.named("preBuild") { dependsOn(sharedAssets) }

dependencies {
    implementation("com.journeyapps:zxing-android-embedded:4.3.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
    testImplementation("org.robolectric:robolectric:4.13")
    testImplementation("io.github.takahirom.roborazzi:roborazzi:1.26.0")
    testImplementation("io.github.takahirom.roborazzi:roborazzi-compose:1.26.0")
    testImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
    implementation(platform("androidx.compose:compose-bom:2024.09.03"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.9.2")
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.6")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")
    // X25519 для ключей WireGuard (в системе доступен только с Android 13).
    implementation("org.bouncycastle:bcprov-jdk18on:1.78.1")
}
