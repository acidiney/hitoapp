import HitoKit
import SwiftUI

struct SignedInView: View {
    @Environment(AppModel.self) private var model
    let address: ServerAddress
    let user: User
    @State private var confirmingChange = false

    var body: some View {
        Screen {
            Heading(title: "Hello, \(user.displayName ?? user.username)")
            Text("Your library and player come next.").foregroundStyle(Theme.soft)
            VStack(alignment: .leading, spacing: 4) {
                Text("This app uses \(address.origin.absoluteString)").foregroundStyle(Theme.soft)
                TextAction(title: "Change…") { confirmingChange = true }
            }
            TextAction(title: "Sign out") { model.signOut() }
        }
        .confirmationDialog("Use a different place?", isPresented: $confirmingChange, titleVisibility: .visible) {
            Button("Change") { model.changeServer() }
        } message: {
            Text("The music stops and the first screen opens. You stay signed in to \(address.host).")
        }
    }
}
