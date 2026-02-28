import SwiftUI

/// A list of editable workspace files with icons and descriptions.
struct WorkspaceEditorView: View {
    @StateObject private var viewModel = WorkspaceViewModel()

    var body: some View {
        NavigationStack {
            Group {
                if viewModel.isLoading {
                    ProgressView("Loading workspace files...")
                } else if viewModel.files.isEmpty {
                    ContentUnavailableView(
                        "No Workspace Files",
                        systemImage: "doc.text",
                        description: Text("Initialize LocalGPT to create workspace files.")
                    )
                } else {
                    List(viewModel.files) { file in
                        NavigationLink(destination: FileEditorView(file: file, viewModel: viewModel)) {
                            HStack(spacing: 12) {
                                Image(systemName: file.iconName)
                                    .font(.title2)
                                    .foregroundColor(file.isSecuritySensitive ? .orange : .teal)
                                    .frame(width: 32)

                                VStack(alignment: .leading, spacing: 4) {
                                    HStack {
                                        Text(file.name)
                                            .font(.headline)

                                        if file.isSecuritySensitive {
                                            Image(systemName: "exclamationmark.shield.fill")
                                                .font(.caption)
                                                .foregroundColor(.orange)
                                        }
                                    }

                                    Text(file.description)
                                        .font(.caption)
                                        .foregroundColor(.secondary)
                                        .lineLimit(2)

                                    if file.content.isEmpty {
                                        Text("Not yet created")
                                            .font(.caption2)
                                            .foregroundColor(.secondary)
                                            .italic()
                                    } else {
                                        Text("\(file.content.count) characters")
                                            .font(.caption2)
                                            .foregroundColor(.secondary)
                                    }
                                }
                            }
                            .padding(.vertical, 4)
                        }
                    }
                }
            }
            .navigationTitle("Workspace")
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button(action: { viewModel.loadFiles() }) {
                        Image(systemName: "arrow.clockwise")
                    }
                }
            }
            .alert("Error", isPresented: $viewModel.showError) {
                Button("OK", role: .cancel) { }
            } message: {
                Text(viewModel.lastError ?? "Unknown error")
            }
        }
    }
}

#Preview {
    WorkspaceEditorView()
}
