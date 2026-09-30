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
        versionCode = 1
        versionName = "0.1.0"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}

dependencies {
    // System QR scanner UI; needs no camera permission.
    implementation("com.google.android.gms:play-services-code-scanner:16.1.0")
}
