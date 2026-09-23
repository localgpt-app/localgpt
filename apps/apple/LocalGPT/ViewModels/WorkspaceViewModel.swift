import Foundation
import Combine

/// Represents an editable workspace file for the UI.
struct WorkspaceFileItem: Identifiable {
    let id: String
    let name: String
    var content: String
    let isSecuritySensitive: Bool

    /// User-friendly description of the file's purpose.
    var description: String {
        switch name {
        case "MEMORY.md":
            return "Long-term curated knowledge the agent remembers across sessions."
        case "SOUL.md":
            return "Persona and tone guidance that shapes how the agent communicates."
        case "HEARTBEAT.md":
            return "Task queue for autonomous background operations."
        case "POLICY.md":
            return "Security policy and standing instructions that shape what the agent does."
        default:
            return "Workspace file."
        }
    }

    /// SF Symbol icon name for the file.
    var iconName: String {
        switch name {
        case "MEMORY.md": return "brain.head.profile"
        case "SOUL.md": return "person.fill"
        case "HEARTBEAT.md": return "heart.fill"
        case "POLICY.md": return "lock.shield.fill"
        default: return "doc.text"
        }
    }
}

@MainActor
class WorkspaceViewModel: ObservableObject {
    @Published var files: [WorkspaceFileItem] = []
    @Published var isLoading = false
    @Published var showError = false
    @Published var lastError: String?
    @Published var showSaveSuccess = false

    private let workspaceURL: URL

    // Default workspace files
    private let defaultFiles: [(name: String, content: String, sensitive: Bool)] = [
        ("MEMORY.md", "# Memory\n\nThis file stores long-term knowledge.\n", false),
        ("SOUL.md", "# Soul\n\nYou are a helpful AI assistant.\n", false),
        ("HEARTBEAT.md", "# Heartbeat\n\nTasks for autonomous operation.\n", false),
        ("POLICY.md", "# Security Policy\n\nThis file is security-sensitive.\n", true)
    ]

    init() {
        let docs = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!
        workspaceURL = docs.appendingPathComponent("LocalGPT/workspace", isDirectory: true)
        loadFiles()
    }

    func loadFiles() {
        isLoading = true

        // Create workspace directory if needed
        if !FileManager.default.fileExists(atPath: workspaceURL.path) {
            try? FileManager.default.createDirectory(at: workspaceURL, withIntermediateDirectories: true)
        }

        var loadedFiles: [WorkspaceFileItem] = []

        for (name, defaultContent, sensitive) in defaultFiles {
            let fileURL = workspaceURL.appendingPathComponent(name)
            let content: String

            if FileManager.default.fileExists(atPath: fileURL.path),
               let existingContent = try? String(contentsOf: fileURL, encoding: .utf8) {
                content = existingContent
            } else {
                // Create file with default content
                try? defaultContent.write(to: fileURL, atomically: true, encoding: .utf8)
                content = defaultContent
            }

            loadedFiles.append(WorkspaceFileItem(
                id: name,
                name: name,
                content: content,
                isSecuritySensitive: sensitive
            ))
        }

        files = loadedFiles
        isLoading = false
    }

    func saveFile(name: String, content: String) {
        let fileURL = workspaceURL.appendingPathComponent(name)

        do {
            try content.write(to: fileURL, atomically: true, encoding: .utf8)

            // Update the local file list
            if let index = files.firstIndex(where: { $0.name == name }) {
                files[index].content = content
            }
            showSaveSuccess = true
        } catch {
            handleError(error)
        }
    }

    func isSecuritySensitive(filename: String) -> Bool {
        filename == "POLICY.md" || filename == "LocalGPT.md"
    }

    private func handleError(_ error: Error) {
        self.lastError = error.localizedDescription
        self.showError = true
    }
}
