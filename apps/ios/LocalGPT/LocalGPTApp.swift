import SwiftUI
import LocalGPTWrapper

@main
struct LocalGPTApp: App {
    init() {
        #if DEBUG
        // Suppress UIKit's internal Auto Layout constraint warnings on iPad
        // These are known Apple bugs in _UIRemoteKeyboardPlaceholderView
        UserDefaults.standard.set(false, forKey: "_UIConstraintBasedLayoutLogUnsatisfiable")
        #endif
    }

    var body: some Scene {
        WindowGroup {
            TabView {
                ChatView()
                    .tabItem {
                        Label("Chat", systemImage: "message.fill")
                    }

                WorkspaceEditorView()
                    .tabItem {
                        Label("Workspace", systemImage: "doc.text.fill")
                    }
            }
            .tint(.teal)
        }
    }
}
