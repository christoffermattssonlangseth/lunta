import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// Google OAuth: an "Android" client id from Google Cloud Console (APIs & Services > Credentials), set in
// android/local.properties as  lunta.googleClientId=1234-abc.apps.googleusercontent.com
// (or the LUNTA_GOOGLE_CLIENT_ID environment variable). Without it the app builds and runs, and
// Google Drive sign-in explains what is missing.
val local = Properties().apply {
    val f = rootProject.file("local.properties")
    if (f.exists()) f.inputStream().use { load(it) }
}
val googleClientId: String = (local.getProperty("lunta.googleClientId") ?: System.getenv("LUNTA_GOOGLE_CLIENT_ID") ?: "").trim()
// the redirect scheme Google expects for Android clients: the client id, reversed
val redirectScheme: String =
    if (googleClientId.endsWith(".apps.googleusercontent.com"))
        "com.googleusercontent.apps." + googleClientId.removeSuffix(".apps.googleusercontent.com")
    else "app.lunta.noclient"

// Release signing: the key made once with keytool (~/.lunta/lunta-android-release.jks, alias "lunta",
// password in the macOS Keychain as "Lunta Android release key"). Given by environment variables (CI:
// decoded from the repository secrets ANDROID_KEYSTORE_BASE64 / ANDROID_KEYSTORE_PASSWORD) or by
// android/local.properties (lunta.keystore=…, lunta.keystorePassword=…). Without it, release builds
// are unsigned. Every update must be signed with the same key: keep a backup of it.
fun setting(env: String, prop: String): String? = (System.getenv(env) ?: local.getProperty(prop))?.trim()?.takeIf { it.isNotEmpty() }
// Updates from GitHub Releases (app/lunta/update/): on, for the APK on GitHub. A build for a store
// (which updates apps itself and doesn't allow REQUEST_INSTALL_PACKAGES for this) sets
// lunta.selfUpdate=false or LUNTA_SELF_UPDATE=false: no update checks, no permission, no provider.
val selfUpdate: Boolean = setting("LUNTA_SELF_UPDATE", "lunta.selfUpdate")?.toBooleanStrictOrNull() ?: true
// The Google Play build (docs/google-play.md): an AAB, with the same application id, key and version
// code as the GitHub APK, made with  LUNTA_SELF_UPDATE=false ./gradlew :app:bundleRelease . An AAB is
// only ever for Play, so one with the updater (and REQUEST_INSTALL_PACKAGES, which Play rejects) is refused.
if (selfUpdate && gradle.startParameter.taskNames.any { it.substringAfterLast(':').startsWith("bundle") }) {
    throw GradleException("An AAB is for Google Play, which updates the app itself: build it with LUNTA_SELF_UPDATE=false (docs/google-play.md)")
}
val releaseKeystore = setting("LUNTA_KEYSTORE", "lunta.keystore")
val releasePassword = setting("LUNTA_KEYSTORE_PASSWORD", "lunta.keystorePassword")

android {
    namespace = "app.lunta"
    compileSdk = 36

    defaultConfig {
        applicationId = "app.lunta"
        minSdk = 26
        targetSdk = 36
        // a release from a tag: its version (v0.2.0 -> "0.2.0") and an always larger code (the CI run)
        versionCode = setting("LUNTA_VERSION_CODE", "lunta.versionCode")?.toInt() ?: 1
        versionName = setting("LUNTA_VERSION_NAME", "lunta.versionName") ?: "1.2.0"
        buildConfigField("String", "GOOGLE_CLIENT_ID", "\"$googleClientId\"")
        buildConfigField("String", "REDIRECT_SCHEME", "\"$redirectScheme\"")
        buildConfigField("boolean", "SELF_UPDATE", selfUpdate.toString())
        manifestPlaceholders["redirectScheme"] = redirectScheme
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    signingConfigs {
        if (releaseKeystore != null && releasePassword != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = releasePassword
                keyAlias = "lunta"
                keyPassword = releasePassword
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false   // JNA and the UniFFI bindings use reflection
            signingConfig = signingConfigs.findByName("release")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        compose = true
        buildConfig = true
    }
    packaging {
        // libpdfium.so is loaded by path (PDFIUM_PATH), so the libraries must be extracted
        jniLibs.useLegacyPackaging = true
    }
    if (selfUpdate) {
        // the install permission and the FileProvider (src/selfupdate/AndroidManifest.xml)
        sourceSets["debug"].manifest.srcFile("src/selfupdate/AndroidManifest.xml")
        sourceSets["release"].manifest.srcFile("src/selfupdate/AndroidManifest.xml")
    }
    testOptions.unitTests.isReturnDefaultValues = true
    sourceSets["main"].apply {
        kotlin.directories.add("src/main/generated/uniffi")
        assets.directories.add(layout.buildDirectory.dir("generated/readerAssets").get().asFile.path)
    }
}

// The reading view's CSS and scripts are the Apple apps' own, extracted from the Swift sources into
// desktop/src/reader/generated by scripts/extract-reader-assets.mjs; copied into the app's assets.
val readerAssets by tasks.registering(Copy::class) {
    from(rootProject.file("../desktop/src/reader/generated")) { include("*.js", "reader.css") }
    into(layout.buildDirectory.dir("generated/readerAssets/reader"))
}
tasks.named("preBuild") { dependsOn(readerAssets) }

dependencies {
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.icons.extended)
    implementation(libs.activity.compose)
    implementation(libs.lifecycle.viewmodel.compose)
    implementation(libs.lifecycle.runtime.compose)
    implementation(libs.core.ktx)
    implementation(libs.browser)
    implementation(libs.webkit)
    implementation(libs.work.runtime)
    implementation(libs.coroutines.android)
    // SVG figures in the figure viewer and grid (BitmapFactory reads only raster images)
    implementation(libs.androidsvg)
    // Listen: the media notification, lock screen and headset controls (MediaSession) for narration
    implementation(libs.media3.session)
    implementation("${libs.jna.get()}@aar")
    testImplementation(libs.junit)
    testImplementation(libs.json)   // org.json for the JVM (Android's is a stub in unit tests)
}
