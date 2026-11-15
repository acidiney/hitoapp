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
            TextAction(title: "Back", icon: "chevron-left", color: Theme.soft) {
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
                    .accessibilityLabel("Server address")
                    .keyboardType(.URL)
                    .textContentType(.URL)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.go)
                    .focused($focused)
                    .disabled(looking)
                    .onSubmit(connect)
                    .onChange(of: input) {
                        fieldProblem = nil
                        problem = []
                        canRetry = false
                    }
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
                problem = ConnectionMessage.lines(for: error, display: typed.display, host: typed.candidates[0].host)
                canRetry = ConnectionMessage.canRetry(error)
                focused = true
            }
        }
    }
}

enum ConnectionMessage {
    static func lines(for error: Error, display: String, host: String) -> [String] {
        switch error as? ServerConnection.Problem {
        case .notHito:
            ["\(display) answered, but it is not a Hito server."]
        case .certificate:
            [
                "The certificate for \(host) can't be checked, so Hito won't send your password there.",
                "Give the server a certificate this device trusts, or use http:// inside your VPN.",
            ]
        case .notReady:
            ["\(display) is still starting. Try again in a moment."]
        default:
            ["No answer from \(display). Check the address, and that this device is on the same network or VPN."]
        }
    }

    static func canRetry(_ error: Error) -> Bool {
        switch error as? ServerConnection.Problem {
        case .notHito, .certificate: false
        default: true
        }
    }
}

struct RecentServers: View {
    @Environment(AppModel.self) private var model
    @State private var checking: URL?
    @State private var problem: [String] = []

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Recent").font(Theme.smallStrong).foregroundStyle(Theme.soft)
            ForEach(model.recent) { server in
                HStack(spacing: 8) {
                    Button {
                        choose(server)
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(server.address.origin.absoluteString).foregroundStyle(Theme.text)
                            Text(detail(server)).font(Theme.small).foregroundStyle(Theme.quiet)
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
            if !problem.isEmpty { Alert(lines: problem) }
        }
    }

    private func detail(_ server: RecentServer) -> String {
        let used = server.used.formatted(.relative(presentation: .named))
        return [server.username, "used \(used)"].compactMap { $0 }.joined(separator: " · ")
    }

    private func choose(_ server: RecentServer) {
        problem = []
        checking = server.id
        Task {
            defer { checking = nil }
            do {
                try await model.choose(server)
            } catch is CancellationError {
            } catch {
                problem = ConnectionMessage.lines(for: error, display: server.address.origin.absoluteString, host: server.address.host)
            }
        }
    }
}
