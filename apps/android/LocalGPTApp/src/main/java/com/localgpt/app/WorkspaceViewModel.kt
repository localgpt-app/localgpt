package com.localgpt.app

import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.localgpt_mobile.*
import java.io.File

/**
 * Represents an editable workspace file for the UI.
 */
data class WorkspaceFileItem(
    val name: String,
    val content: String,
    val isSecuritySensitive: Boolean
) {
    /** User-friendly description of the file's purpose. */
    val description: String
        get() = when (name) {
            "MEMORY.md" -> "Long-term curated knowledge the agent remembers across sessions."
            "SOUL.md" -> "Persona and tone guidance that shapes how the agent communicates."
            "HEARTBEAT.md" -> "Task queue for autonomous background operations."
            "LocalGPT.md" -> "Security policy that restricts what the agent can do. Changes are cryptographically signed."
            else -> "Workspace file."
        }
}

class WorkspaceViewModel : ViewModel() {
    private var client: LocalGPTClient? = null

    val files = mutableStateListOf<WorkspaceFileItem>()
    val isLoading = mutableStateOf(false)
    val errorMessage = mutableStateOf<String?>(null)
    val saveSuccess = mutableStateOf(false)

    fun initialize(dataDir: File) {
        if (client != null) return

        viewModelScope.launch(Dispatchers.IO) {
            try {
                val appDir = File(dataDir, "LocalGPT")
                if (!appDir.exists()) appDir.mkdirs()

                val newClient = LocalGPTClient(appDir.absolutePath)
                client = newClient

                loadFiles()
            } catch (e: Exception) {
                withContext(Dispatchers.Main) {
                    errorMessage.value = "Init error: ${e.localizedMessage}"
                }
            }
        }
    }

    fun loadFiles() {
        val currentClient = client ?: return

        viewModelScope.launch(Dispatchers.IO) {
            withContext(Dispatchers.Main) { isLoading.value = true }
            try {
                val workspaceFiles = currentClient.listWorkspaceFiles()
                withContext(Dispatchers.Main) {
                    files.clear()
                    files.addAll(workspaceFiles.map { file ->
                        WorkspaceFileItem(
                            name = file.name,
                            content = file.content,
                            isSecuritySensitive = file.isSecuritySensitive
                        )
                    })
                    isLoading.value = false
                }
            } catch (e: Exception) {
                withContext(Dispatchers.Main) {
                    isLoading.value = false
                    errorMessage.value = "Load error: ${e.localizedMessage}"
                }
            }
        }
    }

    fun saveFile(name: String, content: String) {
        val currentClient = client ?: return

        viewModelScope.launch(Dispatchers.IO) {
            try {
                currentClient.setWorkspaceFile(filename = name, content = content)
                withContext(Dispatchers.Main) {
                    // Update local state
                    val index = files.indexOfFirst { it.name == name }
                    if (index >= 0) {
                        files[index] = files[index].copy(content = content)
                    }
                    saveSuccess.value = true
                }
            } catch (e: Exception) {
                withContext(Dispatchers.Main) {
                    errorMessage.value = "Save error: ${e.localizedMessage}"
                }
            }
        }
    }
}
