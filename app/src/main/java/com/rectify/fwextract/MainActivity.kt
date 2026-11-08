package com.rectify.fwextract

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                Surface(Modifier.fillMaxSize()) {
                    ExtractorScreen()
                }
            }
        }
    }
}

@Composable
fun ExtractorScreen(vm: ExtractorViewModel = viewModel()) {
    val context = LocalContext.current

    val pickFirmware = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenMultipleDocuments(),
    ) { uris ->
        if (uris.isNotEmpty()) {
            uris.forEach { persist(context, it, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
            vm.scan(uris)
        }
    }

    val pickOutput = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocumentTree(),
    ) { uri ->
        if (uri != null) {
            persist(
                context,
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
            )
            vm.setOutput(uri)
        }
    }

    val logState = rememberLazyListState()
    LaunchedEffect(vm.log.size) {
        if (vm.log.isNotEmpty()) logState.animateScrollToItem(vm.log.lastIndex)
    }

    Column(Modifier.fillMaxSize().padding(12.dp)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(
                text = vm.firmwareLabel,
                modifier = Modifier.weight(1f),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Spacer(Modifier.width(8.dp))
            Button(
                onClick = { pickFirmware.launch(arrayOf("*/*")) },
                enabled = !vm.busy,
            ) { Text("Firmware(s)") }
        }

        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(
                text = vm.outputLabel,
                modifier = Modifier.weight(1f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Spacer(Modifier.width(8.dp))
            Button(
                onClick = { pickOutput.launch(null) },
                enabled = !vm.busy,
            ) { Text("Output") }
        }

        OutlinedTextField(
            value = vm.filter,
            onValueChange = { vm.filter = it },
            label = { Text("Name contains") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )

        Row(Modifier.horizontalScroll(rememberScrollState())) {
            TextButton(onClick = { vm.setMatching(true) }) { Text("Check matching") }
            TextButton(onClick = { vm.setMatching(false) }) { Text("Uncheck matching") }
            TextButton(onClick = { vm.setAll(true) }) { Text("All") }
            TextButton(onClick = { vm.setAll(false) }) { Text("None") }
            TextButton(onClick = { vm.invertAll() }) { Text("Invert") }
        }

        Text(
            text = "${vm.checkedCount} of ${vm.entries.size} checked",
            style = MaterialTheme.typography.labelLarge,
        )

        LazyColumn(Modifier.weight(1f)) {
            items(vm.visible, key = { it.key }) { entry ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clickable { vm.toggle(entry.key) }
                        .padding(vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(
                        checked = vm.checked[entry.key] == true,
                        onCheckedChange = null,
                    )
                    Text(entry.label, style = MaterialTheme.typography.bodySmall)
                }
            }
        }

        if (vm.busy) {
            LinearProgressIndicator(Modifier.fillMaxWidth().padding(vertical = 6.dp))
        }

        Row(verticalAlignment = Alignment.CenterVertically) {
            Button(
                onClick = { vm.extract() },
                enabled = !vm.busy,
            ) { Text("Extract checked") }
            Spacer(Modifier.width(8.dp))
            OutlinedButton(
                onClick = { vm.cancel() },
                enabled = vm.busy,
            ) { Text("Cancel") }
        }

        LazyColumn(
            state = logState,
            modifier = Modifier.fillMaxWidth().height(160.dp),
        ) {
            items(vm.log) { line ->
                Text(line, fontFamily = FontFamily.Monospace, fontSize = 12.sp)
            }
        }
    }
}

private fun persist(context: Context, uri: Uri, flags: Int) {
    try {
        context.contentResolver.takePersistableUriPermission(uri, flags)
    } catch (e: SecurityException) {
        // Not every provider grants persistable access; the session grant is enough here.
    }
}
