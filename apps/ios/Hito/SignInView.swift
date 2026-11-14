import HitoKit
import SwiftUI

struct SignInView: View {
    @Environment(AppModel.self) private var model
    let address: ServerAddress
    let reason: String?
    @State private var username = ""
    @State private var password = ""
    @State private var working = false
    @State private var problem: String?
    @State private var unreachable = false
    @State private var anotherAccount = false
    @FocusState private var focus: AccountField?

    var body: some View {
        Screen {
            let remembered = anotherAccount ? nil : model.remembered(on: address)
            Heading(
                title: remembered == nil ? "Sign in" : "Welcome back",
                address: address.origin.absoluteString + (working ? " · signing in…" : unreachable ? " · no answer" : "")
            )
            if let reason, problem == nil { Text(reason).foregroundStyle(Theme.soft) }
            if let remembered, let name = remembered.username {
                AccountBadge(name: remembered.displayName ?? name, admin: remembered.admin ?? false)
            } else {
                UsernameField(username: $username, focus: $focus)
            }
            PasswordField(password: $password, focus: $focus, submit: submit)
            if let problem { Alert(lines: [problem]) }
            PrimaryButton(title: unreachable ? "Try again" : "Sign in", busyTitle: "Signing in…", busy: working, action: submit)
                .disabled(working)
            Text("Forgot your password? Ask whoever runs this server to set a new one.")
                .font(Theme.small)
                .foregroundStyle(Theme.quiet)
            if remembered != nil {
                TextAction(title: "Use another account") {
                    anotherAccount = true
                    username = ""
                    password = ""
                    problem = nil
                    focus = .username
                }
                TextAction(title: "Forget me on this device") {
                    model.forgetMe(on: address)
                    anotherAccount = true
                    username = ""
                    password = ""
                    focus = .username
                }
            }
            TextAction(title: "Change server") { model.changeServer() }
                .disabled(working)
        }
        .disabled(working)
        .onAppear {
            username = model.remembered(on: address)?.username ?? ""
            focus = username.isEmpty ? .username : .password
        }
    }

    private func submit() {
        guard !username.isEmpty, !password.isEmpty else {
            focus = username.isEmpty ? .username : .password
            return
        }
        problem = nil
        unreachable = false
        working = true
        Task {
            defer { working = false }
            do {
                try await model.signIn(at: address, username: username, password: password)
            } catch AccountSession.Problem.wrongCredentials {
                working = false
                problem = "Wrong username or password."
                password = ""
                focus = .password
            } catch let AccountSession.Problem.tooManyAttempts(message) {
                problem = message
            } catch let AccountSession.Problem.refused(message) {
                problem = message
            } catch is ServerStore.SessionNotKept {
                problem = "This phone couldn't keep you signed in. Try again."
            } catch AccountSession.Problem.certificate {
                problem = "The certificate for \(address.host) can't be checked, so Hito won't send your password there."
            } catch {
                unreachable = true
                problem = "Can't reach \(address.origin.absoluteString). Check that Hito is running and that this device is on the same network."
            }
        }
    }
}
