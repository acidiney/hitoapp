import HitoKit
import SwiftUI

struct RootView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Group {
            switch model.step {
            case .opening:
                Theme.ground.ignoresSafeArea()
            case let .connecting(address):
                ConnectingView(address: address)
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
            case let .locked(address, user):
                LockView(address: address, user: user)
            case let .signedIn(address, user):
                MainView(address: address, user: user)
            }
        }
        .tint(Theme.focus)
    }
}
