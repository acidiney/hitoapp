import HitoKit
import SwiftUI

struct ConnectingView: View {
    @Environment(AppModel.self) private var model
    let address: ServerAddress
    @State private var slow = false

    var body: some View {
        Screen {
            if slow {
                HStack(spacing: 10) {
                    ProgressView().tint(Theme.text)
                    Text("Connecting to \(address.origin.absoluteString)…").foregroundStyle(Theme.soft)
                }
                TextAction(title: "Change server") { model.changeServer() }
            }
        }
        .task {
            try? await Task.sleep(for: .milliseconds(800))
            slow = true
        }
    }
}
