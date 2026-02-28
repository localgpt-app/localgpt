import Foundation
import LocalGPTWrapper

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
        case "LocalGPT.md":
            return "Security policy that restricts what the agent can do. Changes are cryptographically signed."
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
        case "LocalGPT.md": return "lock.shield.fill"
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

    private var client: LocalGptClient?

    init() {
        setupClient()
    }

    private func setupClient() {
        do {
            let docs = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!
            let dataDir = docs.appendingPathComponent("LocalGPT", isDirectory: true).path
            self.client = try LocalGptClient(dataDir: dataDir)
            loadFiles()
        } catch {
            handleError(error)
        }
    }

    func loadFiles() {
        guard let client = client else { return }
        isLoading = true

        let workspaceFiles = client.listWorkspaceFiles()
        files = workspaceFiles.map { file in
            WorkspaceFileItem(
                id: file.name,
                name: file.name,
                content: file.content,
                isSecuritySensitive: file.isSecuritySensitive
            )
        }

        isLoading = false
    }

    func saveFile(name: String, content: String) {
        guard let client = client else { return }

        Task.detached(priority: .userInitiated) { [weak self] in
            do {
                try client.setWorkspaceFile(filename: name, content: content)
                await MainActor.run {
                    // Update the local file list
                    if let index = self?.files.firstIndex(where: { $0.name == name }) {
                        self?.files[index].content = content
                    }
                    self?.showSaveSuccess = true
                }
            } catch {
                await MainActor.run {
                    self?.handleError(error)
                }
            }
        }
    }

    func isSecuritySensitive(filename: String) -> Bool {
        client?.isWorkspaceFileSecuritySensitive(filename: filename) ?? false
    }

    private func handleError(_ error: Error) {
        self.lastError = error.localizedDescription
        self.showError = true
    }
}
