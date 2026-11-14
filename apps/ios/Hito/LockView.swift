import HitoKit
import SwiftUI

struct AccountBadge: View {
    let name: String
    let admin: Bool
    var detail: String?

    var body: some View {
        HStack(spacing: 12) {
            Text(String(name.prefix(1)).uppercased())
                .font(Theme.bodyBold)
                .foregroundStyle(Theme.onAccent)
                .frame(width: 40, height: 40)
                .background(Theme.accent, in: Circle())
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(name).font(Theme.bodyStrong)
                Text([admin ? "admin" : "member", detail].compactMap { $0 }.joined(separator: " · "))
                    .font(Theme.small)
                    .foregroundStyle(Theme.quiet)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

struct LockView: View {
    @Environment(AppModel.self) private var model
    let address: ServerAddress
    let user: User
    @State private var pin = ""
    @State private var working = false
    @State private var problem: String?
    @FocusState private var focused: Bool

    var body: some View {
        Screen {
            Heading(title: "Locked")
            AccountBadge(name: user.displayName ?? user.username, admin: user.admin)
            FieldBox(
                label: "PIN",
                focused: focused,
                hint: "4 to 8 digits. After 5 wrong tries you sign in with your password."
            ) {
                SecureField("", text: $pin)
                    .keyboardType(.numberPad)
                    .textContentType(.oneTimeCode)
                    .focused($focused)
                    .onSubmit(unlock)
                    .frame(minHeight: 44)
            }
            if let problem { Alert(lines: [problem]) }
            PrimaryButton(title: "Unlock", busyTitle: "Unlocking…", busy: working, action: unlock)
                .disabled(working)
            TextAction(title: "Sign out") { model.signOutWhileLocked() }
                .disabled(working)
        }
        .onAppear { focused = true }
    }

    private func unlock() {
        guard !pin.isEmpty, !working else { return }
        problem = nil
        working = true
        Task {
            do {
                let done = try await model.unlock(pin: pin)
                working = false
                if !done {
                    problem = "Wrong PIN."
                    pin = ""
                    focused = true
                }
            } catch {
                working = false
                problem = "Can't reach \(address.origin.absoluteString). Check that Hito is running and that this device is on the same network."
            }
        }
    }
}
