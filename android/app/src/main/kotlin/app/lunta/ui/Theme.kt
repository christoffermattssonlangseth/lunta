package app.lunta.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// Lunta's teal (the icon's background) with the copper of its margin rule as accent.
val Teal = Color(0xFF2C6E7F)
val Copper = Color(0xFFB4532A)
val CopperLight = Color(0xFFD98B5F)

val HighlightColors = mapOf(
    "yellow" to Color(0xFFFAD44D),
    "green" to Color(0xFF8CD17A),
    "blue" to Color(0xFF7AB8F2),
    "pink" to Color(0xFFF58FB3),
)

private val Light = lightColorScheme(
    primary = Teal, onPrimary = Color.White,
    primaryContainer = Color(0xFFCFE6ED), onPrimaryContainer = Color(0xFF0B3340),
    secondary = Color(0xFF4A6670), onSecondary = Color.White,
    secondaryContainer = Color(0xFFD5E4EA), onSecondaryContainer = Color(0xFF0F2229),
    tertiary = Copper, onTertiary = Color.White,
    tertiaryContainer = Color(0xFFFFDBCB), onTertiaryContainer = Color(0xFF3A1100),
    background = Color(0xFFFAF9F6), onBackground = Color(0xFF1B1C1C),
    surface = Color(0xFFFAF9F6), onSurface = Color(0xFF1B1C1C),
    surfaceVariant = Color(0xFFE3EAEC), onSurfaceVariant = Color(0xFF40484B),
    surfaceContainerLowest = Color.White, surfaceContainerLow = Color(0xFFF4F3EF),
    surfaceContainer = Color(0xFFEFEEEA), surfaceContainerHigh = Color(0xFFE9E8E4), surfaceContainerHighest = Color(0xFFE3E2DE),
    outline = Color(0xFF70797C), outlineVariant = Color(0xFFC0C8CB),
)

private val Dark = darkColorScheme(
    primary = Color(0xFF8CC6D6), onPrimary = Color(0xFF00363F),
    primaryContainer = Color(0xFF1E5566), onPrimaryContainer = Color(0xFFCFE6ED),
    secondary = Color(0xFFB2CAD3), onSecondary = Color(0xFF1C333B),
    secondaryContainer = Color(0xFF334A52), onSecondaryContainer = Color(0xFFD5E4EA),
    tertiary = CopperLight, onTertiary = Color(0xFF512100),
    tertiaryContainer = Color(0xFF7B3512), onTertiaryContainer = Color(0xFFFFDBCB),
    background = Color(0xFF121618), onBackground = Color(0xFFE2E3E3),
    surface = Color(0xFF121618), onSurface = Color(0xFFE2E3E3),
    surfaceVariant = Color(0xFF40484B), onSurfaceVariant = Color(0xFFC0C8CB),
    surfaceContainerLowest = Color(0xFF0D1113), surfaceContainerLow = Color(0xFF181D1F),
    surfaceContainer = Color(0xFF1C2123), surfaceContainerHigh = Color(0xFF262B2D), surfaceContainerHighest = Color(0xFF313638),
    outline = Color(0xFF8A9396), outlineVariant = Color(0xFF40484B),
)

@Composable
fun LuntaTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) Dark else Light, content = content)
}
