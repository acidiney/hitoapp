import HitoKit
import SwiftUI

struct NotEncryptedView: View {
    @Environment(AppModel.self) private var model
    let found: FoundServer

    var body: some View {
        Screen {
            TextAction(title: "Change address", icon: "chevron.left") { model.changeAddress() }
            Heading(title: "This connection is not encrypted", address: found.address.origin.absoluteString)
            Text("Anyone on the same network could read what you send, including your password.")
                .foregroundStyle(Theme.soft)
            Text("That is fine at home or inside a VPN. Don't use it over the open internet.")
                .foregroundStyle(Theme.soft)
            PrimaryButton(title: "Connect anyway") { model.connectAnyway(found) }
        }
    }
}
