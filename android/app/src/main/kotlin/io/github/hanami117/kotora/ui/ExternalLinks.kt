package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import io.github.hanami117.kotora.core.licenceLines
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.data.Links
import io.github.hanami117.kotora.ui.theme.Kotora

/**
 * "Open on MyAnimeList" / "Open on AniList" (desktop details.rs `det/open-mal`, profile.rs
 * `profile/open-mal`): a button with the external-link icon.
 */
@Composable
fun OpenOnButton(label: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(8.dp)
    Row(
        modifier.height(34.dp).clip(shape).background(k.raised, shape).border(1.dp, k.borderStrong, shape)
            .clickable(role = Role.Button, onClick = onClick).padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(label, color = k.text, fontSize = 13.sp, fontWeight = FontWeight.Medium)
        Icon(KIcons.External, null, tint = k.dim, modifier = Modifier.size(14.dp))
    }
}

/** About → Links and Support (desktop settings.rs `Row::Links`, `Row::Support`). */
@Composable
fun AboutLinks(onOpenUrl: (String) -> Unit) {
    val k = Kotora.colors
    var licences by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().clickable(role = Role.Button) { licences = true }.padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text("Licences", color = k.text, fontWeight = FontWeight.Medium, fontSize = 14.sp)
            Text("Kotora, the fonts and the parts it is built from", color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
        }
        Text("›", color = k.dim, fontSize = 18.sp)
    }
    if (licences) LicencesDialog { licences = false }
    HorizontalDivider(color = k.border)
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp)) {
        Text("Links", color = k.text, fontWeight = FontWeight.Medium, fontSize = 14.sp)
        Text("The website, and the source code on GitHub.", color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
        Row(Modifier.padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            TextLink("kotora.moe") { onOpenUrl(Links.SITE) }
            TextLink("Source code") { onOpenUrl(Links.SOURCE) }
        }
    }
    HorizontalDivider(color = k.border)
    Row(Modifier.fillMaxWidth().padding(16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Icon(HeartIcon, null, tint = k.accentHi, modifier = Modifier.size(18.dp).padding(top = 1.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Column {
                Text("Support Kotora", color = k.text, fontWeight = FontWeight.Medium, fontSize = 14.sp)
                Text(
                    "Kotora is free and built in my spare time. If you enjoy it, you can support it on Ko-fi.",
                    color = k.dim,
                    fontSize = 12.sp,
                    modifier = Modifier.padding(top = 3.dp),
                )
            }
            OpenOnButton("Support on Ko-fi", { onOpenUrl(Links.SUPPORT) })
        }
    }
}

@Composable
private fun TextLink(text: String, onClick: () -> Unit) {
    val k = Kotora.colors
    Text(
        text,
        color = k.accentHi,
        fontSize = 14.sp,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.clip(RoundedCornerShape(4.dp)).clickable(role = Role.Button, onClick = onClick).padding(vertical = 4.dp),
    )
}

/** The desktop's heart (icons.rs `HEART`), drawn as KIcons are: a 24 grid, 1.6 stroke. */
private val HeartIcon: ImageVector = ImageVector.Builder("heart", 24.dp, 24.dp, 24f, 24f).apply {
    addPath(
        pathData = addPathNodes("M12 20s-7.5-4.6-7.5-10.2A4.3 4.3 0 0 1 12 7.2a4.3 4.3 0 0 1 7.5 2.6C19.5 15.4 12 20 12 20z"),
        fill = null,
        stroke = SolidColor(Color.Black),
        strokeLineWidth = 1.6f,
        strokeLineCap = StrokeCap.Round,
        strokeLineJoin = StrokeJoin.Round,
    )
}.build()

/** About → Licences: every part the app ships, in the desktop's words (the core's list). */
@Composable
private fun LicencesDialog(onClose: () -> Unit) {
    val k = Kotora.colors
    androidx.compose.material3.AlertDialog(
        onDismissRequest = onClose,
        title = { Text("Licences", color = k.text) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                for (l in licenceLines(true)) {
                    Column {
                        Text(l.name, color = k.text, fontSize = 14.sp, fontWeight = FontWeight.Medium)
                        if (l.licence.isNotEmpty()) Text(l.licence, color = k.faint, fontSize = 12.sp)
                    }
                }
            }
        },
        confirmButton = { androidx.compose.material3.TextButton(onClick = onClose) { Text("Close", color = k.accentHi) } },
        containerColor = k.raised,
    )
}
