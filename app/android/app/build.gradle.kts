plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "org.akshara.office"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "org.akshara.office"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        // Uses the version code from pubspec.yaml. When using split APKs, 1000 * ABI_VERSION
        // is added automatically by Flutter. (https://developer.android.com/studio/build/configure-apk-splits#configure-APK-versions)
        // You can force using the value of versionCode by specifying the `-P force-version-code-ignoring-abi=true`
        // flag during build.
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    buildTypes {
        release {
            // TODO: Add your own signing config for the release build.
            // Signing with the debug keys for now, so `flutter run --release` works.
            signingConfig = signingConfigs.getByName("debug")
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}

// === Akshara Rust core =====================================================
// Cross-compiles libakshara.so for each Android ABI with cargo-ndk
// (`cargo install cargo-ndk`; needs the Android NDK) into src/main/jniLibs,
// which Gradle packages automatically. Pass -PskipRustCore to use libraries
// built separately (e.g. by scripts/build-native.sh android).
val rustCoreTask = tasks.register<Exec>("buildRustCore") {
    group = "build"
    description = "Builds the Akshara Rust core for Android."
    workingDir = rootProject.file("../..")
    commandLine(
        "cargo", "ndk",
        "-t", "arm64-v8a", "-t", "armeabi-v7a", "-t", "x86_64",
        "-o", file("src/main/jniLibs").absolutePath,
        "build", "--release", "-p", "akshara-ffi",
    )
    onlyIf { !project.hasProperty("skipRustCore") }
}
tasks.matching { it.name == "preBuild" }.configureEach { dependsOn(rustCoreTask) }
