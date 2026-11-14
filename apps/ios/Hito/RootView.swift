import HitoKit
import SwiftUI

struct RootView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Group {
            switch model.step {
            case .opening:
                Screen { ProgressView().tint(Theme.text) }
            case .choose:
                ChooseLibraryView()
            case .thisPhone:
                ThisPhoneView()
            case .address:
                ServerAddressView()
            case let .unencrypted(found):
                NotEncryptedView(found: found)
            case let .createAdministrator(address):
                SetupView(address: address)
            case let .signIn(address, reason):
                SignInView(address: address, reason: reason)
            case let .unreachable(address):
                UnreachableView(address: address)
            case let .signedIn(address, user):
                SignedInView(address: address, user: user)
            }
        }
        .tint(Theme.focus)
    }
}
