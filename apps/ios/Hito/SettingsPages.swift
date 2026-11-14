import HitoKit
import SwiftUI

struct SettingsGroup<Rows: View>: View {
    let title: String
    @ViewBuilder let rows: Rows

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(Theme.section).frame(minHeight: 44).accessibilityAddTraits(.isHeader)
            VStack(spacing: 0) { rows }
        }
    }
}

struct ProfileView: View {
    @Environment(AppModel.self) private var model
    @State private var name = ""
    @State private var devices: [Device]?
    @State private var problem: String?
    @State private var saved = false
    @State private var confirmingSignOut = false
    @State private var changingPassword = false
    @State private var passwordNote: String?

    var user: User? {
        if case let .signedIn(_, user) = model.step { return user }
        return nil
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                SettingsGroup(title: "Name") {
                    VStack(alignment: .leading, spacing: 8) {
                        TextField("", text: $name, prompt: Text(user?.username ?? "").foregroundStyle(Theme.quiet))
                            .accessibilityLabel("Display name")
                            .padding(.horizontal, 16)
                            .frame(minHeight: 48)
                            .background(Capsule().fill(Theme.field))
                            .overlay(Capsule().strokeBorder(Theme.line))
                            .onChange(of: name) { saved = false }
                        HStack(spacing: 12) {
                            OutlineButton(title: "Save name") { Task { await rename() } }
                                .disabled(name.trimmingCharacters(in: .whitespaces) == (user?.displayName ?? ""))
                            if saved { Text("Saved.").font(Theme.small).foregroundStyle(Theme.soft) }
                        }
                        Text("Leave it blank to show your username, \(user?.username ?? "").").font(Theme.small).foregroundStyle(Theme.quiet)
                    }
                }
                SettingsGroup(title: "Password") {
                    SettingRow(title: "Change your password", detail: passwordNote ?? "Other devices sign out when it changes.") {
                        OutlineButton(title: "Change") { changingPassword = true }
                    }
                }
                SettingsGroup(title: "Devices") {
                    if let devices {
                        ForEach(Array(devices.enumerated()), id: \.element.id) { index, device in
                            SettingRow(title: device.name, detail: device.current ? "This device" : "Last used \(device.lastSeen.formatted(.relative(presentation: .named)))", divided: index < devices.count - 1) {
                                if !device.current {
                                    Button { Task { await signOut(device) } } label: {
                                        Text("Sign out").font(Theme.smallMedium).foregroundStyle(Theme.focus).padding(.horizontal, 12).frame(minHeight: 44)
                                    }
                                    .buttonStyle(.plain)
                                    .padding(.trailing, -12)
                                    .accessibilityLabel("Sign out \(device.name)")
                                }
                            }
                        }
                    } else {
                        ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
                    }
                }
                if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.alert) }
                Button { confirmingSignOut = true } label: {
                    Text("Sign out of this device").font(Theme.bodyMedium).foregroundStyle(Color(hex: 0xf0a08c)).frame(minHeight: 44)
                }
                .buttonStyle(.plain)
                .confirmationDialog("Sign out of this device?", isPresented: $confirmingSignOut, titleVisibility: .visible) {
                    Button("Sign out", role: .destructive) { model.signOut() }
                } message: {
                    Text("The music stops. Sign in again to keep listening here.")
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
        .sheet(isPresented: $changingPassword) {
            PasswordSheet { current, new in
                guard let api = model.api else { return "Sign in again to change your password." }
                do {
                    passwordNote = try await Profile(api: api).changePassword(current: current, new: new)
                    await loadDevices()
                    return nil
                } catch PasswordChange.Problem.wrongPassword {
                    return "The current password isn't right."
                } catch PasswordChange.Problem.tooManyTries {
                    return "Too many tries. Wait a few minutes, then try again."
                } catch {
                    model.handle(error)
                    return "Your password didn't change. Try again."
                }
            }
        }
        .task {
            name = user?.displayName ?? ""
            await loadDevices()
        }
    }

    private func rename() async {
        guard let api = model.api else { return }
        problem = nil
        do {
            let user = try await Profile(api: api).rename(name)
            model.renamed(user)
            name = user.displayName ?? ""
            saved = true
        } catch {
            problem = "Your name didn't save. Try again."
            model.handle(error)
        }
    }

    private func signOut(_ device: Device) async {
        guard let api = model.api else { return }
        problem = nil
        do {
            try await Profile(api: api).signOut(device: device.id)
            devices?.removeAll { $0.id == device.id }
        } catch {
            problem = "\(device.name) is still signed in. Try again."
            model.handle(error)
        }
    }

    private func loadDevices() async {
        guard let api = model.api else { return }
        do {
            devices = try await Profile(api: api).devices()
        } catch {
            problem = "Your devices didn't load."
            devices = []
            model.handle(error)
        }
    }
}

struct PlaybackSettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var order: SourceOrder.Order?
    @State private var names: [String: String] = [:]
    @State private var problem: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                if let order {
                    SettingsGroup(title: "Source order") {
                        Text("Hito plays the first source in this list that has the track.")
                            .font(Theme.small)
                            .foregroundStyle(Theme.quiet)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.bottom, 4)
                        ForEach(Array(order.entries.enumerated()), id: \.element) { index, entry in
                            SettingRow(title: "\(index + 1). \(label(entry).title)", detail: label(entry).detail, divided: index < order.entries.count - 1) {
                                Menu {
                                    Button("Move up") { Task { await move(index, by: -1) } }.disabled(index == 0)
                                    Button("Move down") { Task { await move(index, by: 1) } }.disabled(index == order.entries.count - 1)
                                } label: {
                                    Icon("more-vertical").foregroundStyle(Theme.quiet).frame(width: 44, height: 44).contentShape(Circle())
                                }
                                .accessibilityLabel("Move \(label(entry).title)")
                            }
                        }
                    }
                    SettingsGroup(title: "When a source fails") {
                        SettingRow(title: "Try the next source", detail: "Up to six sources are tried for one play.") {
                            Switch(on: order.fallback, label: "Try the next source if one fails") { on in
                                var next = order
                                next.fallback = on
                                await save(next)
                            }
                        }
                    }
                } else if problem == nil {
                    ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
                }
                if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.alert) }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
        .task { await load() }
    }

    private func label(_ entry: SourceOrder.Entry) -> (title: String, detail: String) {
        switch entry {
        case .localFiles: ("Local files", "Audio in your music folders")
        case .savedCopies: ("Saved copies", "Copies Hito saved for you")
        case .provider(let id): (names[id] ?? "Unavailable plugin", "Plugin")
        }
    }

    private func move(_ index: Int, by offset: Int) async {
        guard var next = order else { return }
        next.move(index, by: offset)
        await save(next)
    }

    private func save(_ next: SourceOrder.Order) async {
        guard let api = model.api else { return }
        problem = nil
        do {
            order = try await SourceOrder(api: api).save(next)
        } catch HitoAPI.Problem.refused {
            problem = "The order changed somewhere else. Here is the latest."
            await load()
        } catch {
            problem = "The order didn't save. Try again."
            model.handle(error)
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            async let current = SourceOrder(api: api).load()
            async let plugins = Plugins(api: api).load()
            let (loaded, state) = try await (current, plugins)
            names = Dictionary(uniqueKeysWithValues: state.connections.map { ($0.id, $0.name) })
            order = loaded
        } catch {
            problem = "Playback settings didn't load."
            model.handle(error)
        }
    }
}

struct AboutView: View {
    @Environment(AppModel.self) private var model
    @State private var confirmingChange = false

    var version: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String ?? "?"
        return "\(short) (\(build))"
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                SettingsGroup(title: "This app") {
                    SettingRow(title: "Version", detail: version) { EmptyView() }
                }
                if case let .signedIn(address, _) = model.step {
                    SettingsGroup(title: "Server") {
                        SettingRow(title: "This app uses", detail: address.origin.absoluteString) {
                            OutlineButton(title: "Change") { confirmingChange = true }
                                .confirmationDialog("Use a different place?", isPresented: $confirmingChange, titleVisibility: .visible) {
                                    Button("Change") { model.changeServer() }
                                } message: {
                                    Text("The music stops and the first screen opens. You stay signed in to this server.")
                                }
                        }
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
    }
}

struct PasswordSheet: View {
    @Environment(\.dismiss) private var dismiss
    let save: (String, String) async -> String?
    @State private var current = ""
    @State private var new = ""
    @State private var again = ""
    @State private var problem: String?
    @State private var saving = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Change your password").font(Theme.playing).accessibilityAddTraits(.isHeader)
            Text("At least 12 characters. A long phrase is easier to remember. Other devices sign out when it changes.")
                .font(Theme.small)
                .foregroundStyle(Theme.soft)
            field("Current password", $current, .password)
            field("New password", $new, .newPassword)
            field("New password again", $again, .newPassword)
            if let problem {
                Text(problem).font(Theme.small).foregroundStyle(Theme.alert)
            }
            Button {
                if let invalid = PasswordChange.problem(new: new, again: again) {
                    problem = invalid
                    return
                }
                Task {
                    saving = true
                    problem = await save(current, new)
                    saving = false
                    if problem == nil { dismiss() }
                }
            } label: {
                Text(saving ? "Changing…" : "Change password")
                    .font(Theme.bodyStrong)
                    .foregroundStyle(Theme.onAccent)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .background(Capsule().fill(Theme.accent))
            }
            .buttonStyle(.plain)
            .disabled(saving || current.isEmpty || new.isEmpty || again.isEmpty)
            Spacer(minLength: 0)
        }
        .padding(20)
        .foregroundStyle(Theme.text)
        .presentationDetents([.large])
        .presentationBackground(Theme.field)
    }

    private func field(_ label: String, _ text: Binding<String>, _ content: UITextContentType) -> some View {
        SecureField("", text: text, prompt: Text(label).foregroundStyle(Theme.quiet))
            .textContentType(content)
            .accessibilityLabel(label)
            .padding(.horizontal, 16)
            .frame(minHeight: 48)
            .background(Capsule().fill(Theme.ground))
    }
}
