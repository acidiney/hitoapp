import HitoKit
import SwiftUI

struct UnreachableView: View {
    @Environment(AppModel.self) private var model
    let address: ServerAddress

    var body: some View {
        Screen {
            Heading(title: "Can't reach your server", address: "\(address.origin.absoluteString) · no answer")
            Text("Check that this device is on the same network or VPN as the server.")
                .foregroundStyle(Theme.soft)
            HStack(spacing: 10) {
                ProgressView().tint(Theme.quiet)
                Text("Trying again every ten seconds.").font(Theme.small).foregroundStyle(Theme.quiet)
            }
            TextAction(title: "Change server") { model.changeServer() }
        }
    }
}
