import com.android.build.api.variant.ApplicationAndroidComponentsExtension
import java.util.Properties
import javax.inject.Inject
import org.gradle.process.ExecOperations

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.roborazzi)
}

// The repo: android/app -> the kotora checkout.
val repoDir: File = rootDir.parentFile
val mobileDir: File = repoDir.resolve("mobile")

// One version for desktop and Android: the workspace version in the root Cargo.toml.
fun cargoVersion(toml: File): String =
    Regex("""(?m)^version\s*=\s*"([^"]+)"""").find(toml.readText())?.groupValues?.get(1)
        ?: error("no version in $toml")

val kotoraVersion = cargoVersion(repoDir.resolve("Cargo.toml"))
check(cargoVersion(mobileDir.resolve("Cargo.toml")) == kotoraVersion) {
    "mobile/Cargo.toml's version must be the desktop's ($kotoraVersion)"
}
// 0.5.0 -> 500, 1.2.13 -> 10213; a fourth number (a re-release of the APK) is added on.
val kotoraVersionCode: Int = kotoraVersion.split('.', '-').take(3).map { it.toIntOrNull() ?: 0 }
    .let { (a, b, c) -> a * 10000 + b * 100 + c }

fun clientId(prop: String, env: String): String =
    (System.getenv(env)?.takeIf { it.isNotBlank() } ?: (findProperty(prop) as String?).orEmpty()).trim()

val malClientId = clientId("kotora.malClientId", "KOTORA_ANDROID_MAL_CLIENT_ID")
val anilistClientId = clientId("kotora.anilistClientId", "KOTORA_ANDROID_ANILIST_CLIENT_ID")

// Release signing, never in the repo. Either the environment (CI: the keystore decoded from a
// secret) or a properties file outside the checkout given with -Pkotora.signing=/path/signing.properties
// (keys storeFile, storePassword, keyAlias, keyPassword). Neither: the release APK is unsigned.
val signingProps: Map<String, String> = run {
    val env = mapOf(
        "storeFile" to System.getenv("KOTORA_ANDROID_KEYSTORE"),
        "storePassword" to System.getenv("KOTORA_ANDROID_KEYSTORE_PASSWORD"),
        "keyAlias" to System.getenv("KOTORA_ANDROID_KEY_ALIAS"),
        "keyPassword" to System.getenv("KOTORA_ANDROID_KEY_PASSWORD"),
    )
    if (env.values.all { !it.isNullOrBlank() }) return@run env.mapValues { it.value!! }
    val file = (findProperty("kotora.signing") as String?)?.let(::File) ?: return@run emptyMap()
    val p = Properties().apply { file.inputStream().use { load(it) } }
    p.stringPropertyNames().associateWith { p.getProperty(it).trim() }
}
val canSignRelease = listOf("storeFile", "storePassword", "keyAlias", "keyPassword").all { !signingProps[it].isNullOrBlank() }

// The ABIs the Rust library is built for: arm64-v8a (phones) and x86_64 (emulators, Chromebooks).
// -Pkotora.abis=arm64-v8a builds one, for a faster local build.
val rustAbis: List<String> =
    (findProperty("kotora.abis") as String?)?.split(',')?.map { it.trim() }?.filter { it.isNotEmpty() }
        ?: listOf("arm64-v8a", "x86_64")

android {
    namespace = "io.github.hanami117.kotora"
    // The current AndroidX libraries want API 37 to compile against; the app still targets 36
    // (what Play asks), so no API 37 behaviour changes apply.
    compileSdk {
        version = release(37) { minorApiLevel = 2 }
    }
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "io.github.hanami117.kotora"
        minSdk = 26
        targetSdk = 36
        versionCode = kotoraVersionCode
        versionName = kotoraVersion
        buildConfigField("String", "MAL_CLIENT_ID", "\"$malClientId\"")
        buildConfigField("String", "ANILIST_CLIENT_ID", "\"$anilistClientId\"")
        ndk { abiFilters += rustAbis }
    }

    // English only for now: the libraries' own translations would otherwise add ~80 locales.
    androidResources {
        localeFilters += "en"
    }

    signingConfigs {
        if (canSignRelease) {
            create("release") {
                storeFile = file(signingProps.getValue("storeFile"))
                storePassword = signingProps.getValue("storePassword")
                keyAlias = signingProps.getValue("keyAlias")
                keyPassword = signingProps.getValue("keyPassword")
                enableV1Signing = false
                enableV2Signing = true
                enableV3Signing = true
            }
        }
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".debug"
            versionNameSuffix = "-debug"
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            if (canSignRelease) signingConfig = signingConfigs.getByName("release")
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs { useLegacyPackaging = false }
    }

    testOptions {
        unitTests {
            isIncludeAndroidResources = true
            all { it.systemProperty("robolectric.pixelCopyRenderMode", "hardware") }
        }
    }

    lint {
        abortOnError = true
        warningsAsErrors = false
        disable += setOf("GradleDependency", "NewerVersionAvailable", "AndroidGradlePluginVersion", "OldTargetApi")
    }
}

// ------------------------------------------------------------------------------------- Rust

/** The Rust sources the library is built from: a change in any of them builds it again. */
val rustSources = files(
    fileTree(repoDir.resolve("core/src")),
    repoDir.resolve("core/Cargo.toml"),
    repoDir.resolve("Cargo.toml"),
    fileTree(mobileDir.resolve("src")),
    mobileDir.resolve("Cargo.toml"),
    mobileDir.resolve("Cargo.lock"),
    mobileDir.resolve("uniffi.toml"),
)

/** `cargo ndk`: libkotora_mobile.so for each ABI, release profile (small, fast; debug APKs too). */
abstract class CargoNdk @Inject constructor(private val ops: ExecOperations) : DefaultTask() {
    @get:InputFiles @get:PathSensitive(PathSensitivity.RELATIVE) abstract val sources: ConfigurableFileCollection
    @get:Input abstract val abis: ListProperty<String>
    @get:Input abstract val minSdk: Property<Int>
    @get:Internal abstract val crateDir: DirectoryProperty
    @get:Internal abstract val ndkDir: DirectoryProperty
    @get:OutputDirectory abstract val outDir: DirectoryProperty

    @TaskAction
    fun build() {
        val out = outDir.get().asFile
        out.deleteRecursively()
        ops.exec {
            workingDir = crateDir.get().asFile
            environment("ANDROID_NDK_HOME", ndkDir.get().asFile.path)
            val targets = abis.get().flatMap { listOf("-t", it) }
            commandLine(listOf("cargo", "ndk") + targets + listOf("-P", minSdk.get().toString(), "-o", out.path, "build", "--release", "--locked", "-p", "kotora-mobile"))
        }
    }
}

/**
 * The Kotlin bindings, from a host build of the same crate (the Android .so is stripped of
 * UniFFI's metadata; the host library has the same).
 */
abstract class UniffiBindings @Inject constructor(private val ops: ExecOperations) : DefaultTask() {
    @get:InputFiles @get:PathSensitive(PathSensitivity.RELATIVE) abstract val sources: ConfigurableFileCollection
    @get:Internal abstract val crateDir: DirectoryProperty
    @get:OutputDirectory abstract val outDir: DirectoryProperty

    @TaskAction
    fun generate() {
        val dir = crateDir.get().asFile
        val out = outDir.get().asFile
        out.deleteRecursively()
        ops.exec { workingDir = dir; commandLine("cargo", "build", "--locked", "-p", "kotora-mobile") }
        val os = System.getProperty("os.name").lowercase()
        val lib = when {
            os.contains("mac") -> "libkotora_mobile.dylib"
            os.contains("windows") -> "kotora_mobile.dll"
            else -> "libkotora_mobile.so"
        }
        ops.exec {
            workingDir = dir
            commandLine("cargo", "run", "--locked", "-q", "-p", "uniffi-bindgen", "--", "generate", "--library", "target/debug/$lib", "--language", "kotlin", "--out-dir", out.path, "--no-format")
        }
    }
}

val androidComponents = extensions.getByType(ApplicationAndroidComponentsExtension::class.java)

val cargoNdk = tasks.register<CargoNdk>("cargoNdk") {
    sources.from(rustSources)
    abis.set(rustAbis)
    minSdk.set(26)
    crateDir.set(mobileDir)
    ndkDir.set(androidComponents.sdkComponents.ndkDirectory)
    outDir.set(layout.buildDirectory.dir("rust/jniLibs"))
}

val uniffiBindings = tasks.register<UniffiBindings>("uniffiBindings") {
    sources.from(rustSources)
    crateDir.set(mobileDir)
    outDir.set(layout.buildDirectory.dir("generated/uniffi/kotlin"))
}

androidComponents.onVariants { variant ->
    variant.sources.jniLibs?.addGeneratedSourceDirectory(cargoNdk, CargoNdk::outDir)
    variant.sources.kotlin?.addGeneratedSourceDirectory(uniffiBindings, UniffiBindings::outDir)
}

// ------------------------------------------------------------------------------ dependencies

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.core.splashscreen)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.browser)
    implementation(libs.androidx.work.runtime)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.coil.compose)
    implementation(libs.coil.network)
    implementation(libs.kotlinx.coroutines.android)
    implementation("${libs.jna.get()}@aar")
    debugImplementation(libs.compose.ui.tooling)
    debugImplementation(libs.compose.ui.test.manifest)

    testImplementation(libs.junit)
    testImplementation(libs.robolectric)
    testImplementation(libs.androidx.test.junit)
    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
    testImplementation(libs.roborazzi)
    testImplementation(libs.roborazzi.compose)
    testImplementation(libs.roborazzi.junit.rule)
}
