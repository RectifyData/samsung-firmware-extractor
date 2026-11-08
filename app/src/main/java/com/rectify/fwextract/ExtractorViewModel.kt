package com.rectify.fwextract

import android.app.Application
import android.net.Uri
import android.provider.OpenableColumns
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.documentfile.provider.DocumentFile
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.io.IOException
import java.util.Locale

class ExtractorViewModel(app: Application) : AndroidViewModel(app) {

    data class Entry(
        val key: String,
        val label: String,
        val firmware: Uri,
        val fwName: String,
        val path: String,
    )

    var firmwareLabel by mutableStateOf("No firmware selected")
        private set
    var outputLabel by mutableStateOf("No output folder")
        private set
    var filter by mutableStateOf("")
    var busy by mutableStateOf(false)
        private set

    val entries = mutableStateListOf<Entry>()
    val checked = mutableStateMapOf<String, Boolean>()
    val log = mutableStateListOf<String>()

    private var outputTree: Uri? = null
    private var job: Job? = null
    private val context get() = getApplication<Application>()

    val visible: List<Entry>
        get() = entries.filter { filter.isBlank() || it.label.contains(filter, ignoreCase = true) }

    val checkedCount: Int
        get() = entries.count { checked[it.key] == true }

    fun setOutput(uri: Uri) {
        outputTree = uri
        outputLabel = uri.lastPathSegment ?: uri.toString()
    }

    fun scan(uris: List<Uri>) {
        if (job?.isActive == true) return
        job = viewModelScope.launch(Dispatchers.IO) {
            onMain {
                busy = true
                firmwareLabel = uris.joinToString("; ") { displayName(it) }
            }
            val found = mutableListOf<Entry>()
            for (uri in uris) {
                val name = displayName(uri)
                say("Scanning $name ...")
                try {
                    var count = 0
                    for (line in Native.scan(openFd(uri), name).lineSequence()) {
                        if (line.isBlank()) continue
                        val (size, path) = line.split('\t', limit = 2)
                        found += Entry(
                            key = "$uri|$path",
                            label = "$name  >  $path" + sizeLabel(size.toLong()),
                            firmware = uri,
                            fwName = name,
                            path = path,
                        )
                        count++
                    }
                    say("  $count lz4 image(s)")
                } catch (e: Exception) {
                    say("  scan failed: ${e.message}")
                }
            }
            onMain {
                entries.clear()
                entries.addAll(found)
                checked.clear()
                for (entry in found) checked[entry.key] = true
                busy = false
            }
        }
    }

    fun extract() {
        val tree = outputTree
        if (tree == null) {
            say("Choose an output folder first.")
            return
        }
        val groups = entries.filter { checked[it.key] == true }.groupBy { it.firmware }
        if (groups.isEmpty()) {
            say("Nothing checked.")
            return
        }
        if (job?.isActive == true) return

        job = viewModelScope.launch(Dispatchers.IO) {
            onMain { busy = true }
            try {
                val root = DocumentFile.fromTreeUri(context, tree)
                    ?: throw IOException("output folder is not accessible")

                for ((uri, group) in groups) {
                    val fwName = group.first().fwName
                    val folder = fwName.substringBefore('.') + "_img"
                    val dir = root.findFile(folder)?.takeIf { it.isDirectory }
                        ?: root.createDirectory(folder)
                        ?: throw IOException("cannot create folder $folder")

                    val selected = group.joinToString("\n") { it.path }
                    say("== $fwName: ${group.size} image(s) -> $folder")
                    val work = File(context.cacheDir, "fwx").absolutePath

                    Native.extract(openFd(uri), fwName, selected, work, object : Native.Callback {
                        override fun onLog(message: String) {
                            say(message)
                        }

                        override fun onImage(name: String, path: String) {
                            publish(dir, name, File(path))
                        }
                    })
                }
                say("Done.")
            } catch (e: Exception) {
                val cancelled = e.message?.contains("cancelled") == true
                say(if (cancelled) "Cancelled." else "Error: ${e.message}")
            } finally {
                onMain { busy = false }
            }
        }
    }

    fun cancel() {
        Native.cancel()
    }

    fun toggle(key: String) {
        checked[key] = checked[key] != true
    }

    fun setMatching(value: Boolean) {
        for (entry in visible) checked[entry.key] = value
    }

    fun setAll(value: Boolean) {
        for (entry in entries) checked[entry.key] = value
    }

    fun invertAll() {
        for (entry in entries) checked[entry.key] = checked[entry.key] != true
    }

    private fun publish(dir: DocumentFile, name: String, local: File) {
        dir.findFile(name)?.delete()
        val doc = dir.createFile("application/octet-stream", name)
            ?: throw IOException("cannot create $name")
        val out = context.contentResolver.openOutputStream(doc.uri)
            ?: throw IOException("cannot write $name")
        out.use { sink -> local.inputStream().use { it.copyTo(sink, 1 shl 20) } }
        val savedAs = doc.name
        if (savedAs != null && savedAs != name) say("  saved as $savedAs")
    }

    private fun openFd(uri: Uri): Int {
        val pfd = context.contentResolver.openFileDescriptor(uri, "r")
            ?: throw IOException("cannot open $uri")
        return pfd.detachFd()
    }

    private fun displayName(uri: Uri): String {
        context.contentResolver
            .query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { cursor ->
                if (cursor.moveToFirst()) return cursor.getString(0)
            }
        return uri.lastPathSegment ?: "firmware"
    }

    private fun sizeLabel(bytes: Long): String =
        if (bytes >= 0) String.format(Locale.US, "  %.1f MiB", bytes / 1048576.0) else ""

    private fun say(message: String) {
        viewModelScope.launch(Dispatchers.Main) { log.add(message) }
    }

    private suspend fun onMain(block: () -> Unit) {
        withContext(Dispatchers.Main) { block() }
    }
}
