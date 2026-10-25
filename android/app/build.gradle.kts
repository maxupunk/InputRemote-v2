import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "io.github.inputremote.painel"
    compileSdk = 36
    // O mesmo NDK que compila o núcleo em Rust (`cargo ndk`, abaixo).
    ndkVersion = "27.2.12479018"

    defaultConfig {
        applicationId = "io.github.inputremote.painel"
        // Android 8: os tablets velhos são os que viram painel (docs/12, §2).
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
    }

    // A chave do release vem de fora (`empacotar/empacotar.ps1` a passa com -P): nenhuma senha mora
    // no repositório. Sem ela, o release sai sem assinatura, e o Android não o instala.
    val chave = providers.gradleProperty("painel.chave").orNull
    signingConfigs {
        if (chave != null) {
            create("painel") {
                storeFile = file(chave)
                storePassword = providers.gradleProperty("painel.senha").get()
                keyAlias = providers.gradleProperty("painel.apelido").get()
                keyPassword = storePassword
            }
        }
    }

    buildTypes {
        release {
            signingConfigs.findByName("painel")?.let { signingConfig = it }
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDir(layout.buildDirectory.dir("nucleo"))
        }
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

// O núcleo em Rust (`crates/ir-monitor-android`), compilado para as três arquiteturas que importam:
// arm64 (quase todo aparelho), armv7 (os tablets velhos) e x86_64 (o emulador). O perfil `android`
// é o `release` com `unwind`: um pânico vira exceção Java, e não derruba o aplicativo.
val compilarNucleo by tasks.registering(Exec::class) {
    val raiz = rootProject.projectDir.parentFile
    val saida = layout.buildDirectory.dir("nucleo").get().asFile
    workingDir = raiz
    environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)
    commandLine(
        "cargo", "ndk",
        "-t", "arm64-v8a", "-t", "armeabi-v7a", "-t", "x86_64",
        "--platform", "26",
        "-o", saida.absolutePath,
        "build", "--profile", "android", "-p", "ir-monitor-android",
    )
    inputs.dir(File(raiz, "crates"))
    inputs.file(File(raiz, "Cargo.lock"))
    outputs.dir(saida)
}

tasks.named("preBuild") {
    dependsOn(compilarNucleo)
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.ui)
    implementation(libs.androidx.ui.graphics)
    implementation(libs.androidx.ui.tooling.preview)
    implementation(libs.androidx.material3)
    debugImplementation(libs.androidx.ui.tooling)

    testImplementation(libs.junit)
    testImplementation(libs.json)
}
