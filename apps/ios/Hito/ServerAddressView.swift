import HitoKit
import SwiftUI

struct ServerAddressView: View {
    @Environment(AppModel.self) private var model
    @State private var input = ""
    @State private var search: Task<Void, Never>?
    @State private var looking = false
    @State private var fieldProblem: String?
    @State private var problem: [String] = []
    @State private var canRetry = false
    @FocusState private var focused: Bool

    var body: some View {
        Screen {
            TextAction(title: "Back", icon: "chevron.left", color: Theme.soft) {
                search?.cancel()
                model.backToChoice()
            }
            Heading(title: "Connect to a server")
            FieldBox(
                label: "Server address",
                focused: focused,
                hint: fieldProblem ?? "The address you open in a browser. You can leave out http:// or https://."
            ) {
                TextField("", text: $input, prompt: Text("hito.home:8080").foregroundStyle(Theme.quiet))
                    .keyboardType(.URL)
                    .textContentType(.URL)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.go)
                    .focused($focused)
                    .disabled(looking)
                    .onSubmit(connect)
                    .onChange(of: input) { fieldProblem = nil }
                    .frame(minHeight: 44)
            }
            if !problem.isEmpty { Alert(lines: problem) }
            if looking {
                HStack {
                    ProgressView().tint(Theme.text)
                    Text("Looking for Hito…").foregroundStyle(Theme.soft)
                    Spacer()
                    TextAction(title: "Cancel") { search?.cancel() }
                }
            } else {
                PrimaryButton(title: canRetry ? "Try again" : "Connect", action: connect)
            }
            if !model.recent.isEmpty && !looking {
                RecentServers()
            }
        }
        .onAppear { focused = model.recent.isEmpty }
    }

    private func connect() {
        fieldProblem = nil
        problem = []
        canRetry = false
        let typed: ServerAddressInput
        do {
            typed = try ServerAddressInput(parsing: input)
        } catch {
            fieldProblem = "Type an address like hito.home:8080 or 192.168.1.20:8080."
            focused = true
            return
        }
        looking = true
        search = Task {
            defer { looking = false }
            do {
                try await model.find(typed)
            } catch is CancellationError {
                focused = true
            } catch {
                show(error, display: typed.display, host: typed.candidates[0].host)
            }
        }
    }

    private func show(_ error: Error, display: String, host: String) {
        switch error as? ServerConnection.Problem {
        case .notHito:
            problem = ["\(display) answered, but it is not a Hito server."]
        case .certificate:
            problem = [
                "The certificate for \(host) can't be checked, so Hito won't send your password there.",
                "Give the server a certificate this device trusts, or use http:// inside your VPN.",
            ]
        case .notReady:
            problem = ["\(display) is still starting. Try again in a moment."]
            canRetry = true
        default:
            problem = ["No answer from \(display). Check the address, and that this device is on the same network or VPN."]
            canRetry = true
        }
        focused = true
    }
}

struct RecentServers: View {
    @Environment(AppModel.self) private var model
    @State private var checking: URL?
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Recent").font(.subheadline.weight(.semibold)).foregroundStyle(Theme.soft)
            ForEach(model.recent) { server in
                HStack(spacing: 8) {
                    Button {
                        choose(server)
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(server.address.origin.absoluteString).foregroundStyle(Theme.text)
                            Text(detail(server)).font(.subheadline).foregroundStyle(Theme.quiet)
                        }
                        .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .disabled(checking != nil)
                    if checking == server.id {
                        ProgressView().tint(Theme.text)
                    }
                    TextAction(title: "Forget") { model.forget(server) }
                        .accessibilityLabel("Forget \(server.address.origin.absoluteString)")
                }
            }
            if let problem { Alert(lines: [problem]) }
        }
    }

    private func detail(_ server: RecentServer) -> String {
        let used = server.used.formatted(.relative(presentation: .named))
        return [server.username, "used \(used)"].compactMap { $0 }.joined(separator: " · ")
    }

    private func choose(_ server: RecentServer) {
        problem = nil
        checking = server.id
        Task {
            defer { checking = nil }
            do {
                try await model.choose(server)
            } catch {
                problem = "No answer from \(server.address.origin.absoluteString). Check that this device is on the same network or VPN."
            }
        }
    }
}
