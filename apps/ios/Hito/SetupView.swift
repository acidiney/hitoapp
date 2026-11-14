import HitoKit
import SwiftUI

struct SetupView: View {
    @Environment(AppModel.self) private var model
    let address: ServerAddress
    @State private var username = ""
    @State private var password = ""
    @State private var working = false
    @State private var problem: String?
    @FocusState private var focus: AccountField?

    var body: some View {
        Screen {
            SetupSteps()
            VStack(alignment: .leading, spacing: 6) {
                Heading(title: "Set up Hito")
                Text("This server has no accounts yet. The first one becomes the administrator and can add other people.")
                    .foregroundStyle(Theme.soft)
                AddressLine(text: address.origin.absoluteString)
            }
            UsernameField(username: $username, focus: $focus)
            PasswordField(password: $password, newPassword: true, hint: PasswordRule.progress(password), focus: $focus, submit: submit)
            if let problem { Alert(lines: [problem]) }
            PrimaryButton(title: "Create administrator", busyTitle: "Creating…", busy: working, action: submit)
                .disabled(working)
            TextAction(title: "Change server") { model.changeServer() }
                .disabled(working)
        }
        .disabled(working)
        .onAppear { focus = .username }
    }

    private func submit() {
        problem = nil
        guard !username.isEmpty else {
            focus = .username
            return
        }
        if let rule = PasswordRule.problem(password) {
            problem = rule
            focus = .password
            return
        }
        working = true
        Task {
            defer { working = false }
            do {
                try await model.createAdministrator(at: address, username: username, password: password)
            } catch AccountSession.Problem.alreadySetUp {
                model.alreadySetUp(address)
            } catch let AccountSession.Problem.refused(message) {
                problem = message
            } catch let AccountSession.Problem.tooManyAttempts(message) {
                problem = message
            } catch AccountSession.Problem.certificate {
                problem = "The certificate for \(address.host) can't be checked, so Hito won't send your password there."
            } catch {
                problem = "Can't reach \(address.origin.absoluteString). Check that Hito is running and that this device is on the same network."
            }
        }
    }
}

private struct SetupSteps: View {
    var body: some View {
        HStack(spacing: 10) {
            HStack(spacing: 8) {
                Circle().fill(Theme.text).frame(width: 9, height: 9)
                Text("1. Your account").font(Theme.smallStrong).foregroundStyle(Theme.text)
            }
            .accessibilityElement(children: .combine)
            .accessibilityValue("Current step")
            Rectangle().fill(Theme.line).frame(width: 24, height: 1).accessibilityHidden(true)
            HStack(spacing: 8) {
                Circle().strokeBorder(Color(red: 0x6b / 255, green: 0x6a / 255, blue: 0x65 / 255), lineWidth: 1.5).frame(width: 9, height: 9)
                Text("2. Music folders").font(Theme.small).foregroundStyle(Theme.quiet)
            }
            .accessibilityElement(children: .combine)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Setup steps")
    }
}
