package app.lunta.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Cloud
import androidx.compose.material.icons.filled.PhoneAndroid
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.lunta.AppModel
import app.lunta.R

/** First run: where does the library live? */
@Composable
fun SetupScreen(model: AppModel) {
    val ctx = LocalContext.current
    Column(
        Modifier.fillMaxSize().systemBarsPadding().verticalScroll(rememberScrollState()).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.height(24.dp))
        Image(painterResource(R.drawable.ic_launcher_art), null, Modifier.size(104.dp).clip(RoundedCornerShape(24.dp)))
        Spacer(Modifier.height(16.dp))
        Text("Welcome to Lunta", style = MaterialTheme.typography.headlineMedium)
        Spacer(Modifier.height(8.dp))
        Text(
            "Your papers, readable on the phone. Choose where your library lives.",
            style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(28.dp))

        ElevatedCard(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(20.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Filled.Cloud, null, tint = MaterialTheme.colorScheme.primary)
                    Spacer(Modifier.width(12.dp))
                    Text("Google Drive", style = MaterialTheme.typography.titleLarge)
                }
                Spacer(Modifier.height(8.dp))
                Text(
                    "Keeps the library in a “Lunta” folder in your Drive, in sync with your Mac or PC (Google Drive for desktop). Papers are downloaded when you open them.",
                    style = MaterialTheme.typography.bodyMedium,
                )
                if (model.clientIdMissing) {
                    Spacer(Modifier.height(10.dp))
                    Text(
                        "Sign-in isn't available in this build: no Google OAuth client id was set (android/local.properties, lunta.googleClientId).",
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.tertiary,
                    )
                }
                Spacer(Modifier.height(16.dp))
                Button({ model.beginGoogleSignIn(ctx) }, Modifier.fillMaxWidth()) { Text("Sign in with Google") }
            }
        }
        Spacer(Modifier.height(16.dp))
        ElevatedCard(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(20.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Filled.PhoneAndroid, null, tint = MaterialTheme.colorScheme.primary)
                    Spacer(Modifier.width(12.dp))
                    Text("On this phone", style = MaterialTheme.typography.titleLarge)
                }
                Spacer(Modifier.height(8.dp))
                Text(
                    "A library that stays on this device. Good for trying Lunta out; you can't sync it yet.",
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(Modifier.height(16.dp))
                OutlinedButton({ model.chooseLocal() }, Modifier.fillMaxWidth()) { Text("Use a local library") }
            }
        }
    }
}
