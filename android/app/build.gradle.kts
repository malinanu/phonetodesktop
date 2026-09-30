plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "app.phoneremote"
    compileSdk = 35
    defaultConfig {
        applicationId = "app.phoneremote"
        minSdk = 28 // BluetoothHidDevice needs API 28
        targetSdk = 35
        // CI passes these from the release tag: -PversionName=1.2.3 -PversionCode=10203
        versionCode = (findProperty("versionCode") as String?)?.toInt() ?: 1
        versionName = (findProperty("versionName") as String?) ?: "0.1.0"
    }
    signingConfigs {
        // Release keystore comes from the environment (CI secrets); never commit it.
        val path = System.getenv("ANDROID_KEYSTORE_PATH")
        if (path != null) {
            create("release") {
                storeFile = file(path)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }
    buildTypes {
        release {
            // Without a keystore the APK is debug-signed so it still installs for testing.
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    // The guide page, stylesheet and font are shared with the PC agent (repo /shared).
    sourceSets["main"].assets.srcDir("../../shared")
}

dependencies {
    // System QR scanner UI; needs no camera permission.
    implementation("com.google.android.gms:play-services-code-scanner:16.1.0")
}
