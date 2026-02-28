import SwiftUI

/// A text editor for viewing and editing a single workspace file.
///
/// Security-sensitive files (like LocalGPT.md) show a warning banner
/// and require explicit confirmation before saving.
struct FileEditorView: View {
    let file: WorkspaceFileItem
    @ObservedObject var viewModel: WorkspaceViewModel

    @State private var editedContent: String = ""
    @State private var showSecurityConfirmation = false
    @State private var hasChanges = false
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            // Security warning banner for sensitive files
            if file.isSecuritySensitive {
                securityBanner
            }

            // File description
            HStack {
                Image(systemName: file.iconName)
                    .foregroundColor(file.isSecuritySensitive ? .orange : .teal)
                Text(file.description)
                    .font(.caption)
                    .foregroundColor(.secondary)
                Spacer()
            }
            .padding(.horizontal)
            .padding(.vertical, 8)
            .background(Color(.systemGray6))

            // Text editor
            TextEditor(text: $editedContent)
                .font(.system(.body, design: .monospaced))
                .padding(4)
                .onChange(of: editedContent) { _, newValue in
                    hasChanges = newValue != file.content
                }
        }
        .navigationTitle(file.name)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .navigationBarTrailing) {
                Button("Save") {
                    if file.isSecuritySensitive {
                        showSecurityConfirmation = true
                    } else {
                        saveFile()
                    }
                }
                .disabled(!hasChanges)
                .fontWeight(.semibold)
            }
        }
        .onAppear {
            editedContent = file.content
        }
        .confirmationDialog(
            "Modify Security Policy?",
            isPresented: $showSecurityConfirmation,
            titleVisibility: .visible
        ) {
            Button("Save & Re-sign Policy", role: .destructive) {
                saveFile()
            }
            Button("Cancel", role: .cancel) { }
        } message: {
            Text("You are editing \(file.name), which controls the agent's security restrictions. The file will be cryptographically re-signed after saving. Only make changes you fully understand.")
        }
        .alert("Saved", isPresented: $viewModel.showSaveSuccess) {
            Button("OK", role: .cancel) {
                hasChanges = false
            }
        } message: {
            if file.isSecuritySensitive {
                Text("\(file.name) has been saved and re-signed successfully.")
            } else {
                Text("\(file.name) has been saved successfully.")
            }
        }
        .alert("Error", isPresented: $viewModel.showError) {
            Button("OK", role: .cancel) { }
        } message: {
            Text(viewModel.lastError ?? "Unknown error")
        }
    }

    private var securityBanner: some View {
        HStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundColor(.orange)
            Text("Security-sensitive file. Changes affect agent restrictions and will be re-signed.")
                .font(.caption)
                .foregroundColor(.orange)
            Spacer()
        }
        .padding()
        .background(Color.orange.opacity(0.1))
    }

    private func saveFile() {
        viewModel.saveFile(name: file.name, content: editedContent)
    }
}
