plugins {
    id("com.android.library") version "8.7.3"
    id("org.jetbrains.kotlin.android") version "2.1.21"
}

group = "com.jiangcheng.memory"
version = "0.1.0"

android {
    namespace = "com.jiangcheng.memory.android"
    compileSdk = 35

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")
        ndk {
            abiFilters += setOf("arm64-v8a", "armeabi-v7a", "x86_64", "x86")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")
    sourceSets["main"].java.srcDir("../../java/src/main/kotlin")
    sourceSets["main"].manifest.srcFile("src/main/AndroidManifest.xml")
}

kotlin {
    jvmToolchain(17)
    sourceSets.getByName("main").kotlin.srcDir("../../java/src/main/kotlin")
}

dependencies {
    implementation("net.java.dev.jna:jna:5.17.0@aar")
}

tasks.register("verifyNativeInputs") {
    group = "verification"
    description = "检查四类 Android ABI 的 native 输入目录，不生成或下载二进制。"
    doLast {
        val abiNames = listOf("arm64-v8a", "armeabi-v7a", "x86_64", "x86")
        val root = file("src/main/jniLibs")
        val missing = abiNames.filterNot { file("$root/$it/libjc_memory_uniffi.so").isFile }
        if (missing.isNotEmpty()) {
            throw GradleException(
                "缺少 memory-uniffi Android native slices: ${missing.joinToString()}; " +
                    "请先由目标 ABI 构建流程填充 jniLibs。"
            )
        }
    }
}
