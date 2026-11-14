import HitoKit
import SwiftUI

enum SettingsPage: Hashable {
    case root
    case profile
    case privacy
    case playback
    case about

    var title: String {
        switch self {
        case .root: "Settings"
        case .profile: "Profile"
        case .privacy: "Privacy & lock"
        case .playback: "Playback"
        case .about: "About"
        }
    }
}

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var search = ""

    private let sections: [(page: SettingsPage, title: String, detail: String)] = [
        (.profile, "Profile", "Name, password, devices"),
        (.privacy, "Privacy & lock", "PIN, history, what Hito sends out"),
        (.playback, "Playback", "Source order"),
        (.about, "About", "Version and server"),
    ]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Text("Settings").font(Theme.title).accessibilityAddTraits(.isHeader)
                HStack(spacing: 10) {
                    Icon("search").foregroundStyle(Theme.quiet)
                    TextField("", text: $search, prompt: Text("Search settings").foregroundStyle(Theme.quiet))
                        .accessibilityLabel("Search settings")
                        .autocorrectionDisabled()
                }
                .padding(.horizontal, 16)
                .frame(minHeight: 44)
                .background(Capsule().fill(Theme.field))
                if case let .signedIn(_, user) = model.step {
                    account(user)
                    rows
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
    }

    private func account(_ user: User) -> some View {
        HStack(spacing: 14) {
            Text(String((user.displayName ?? user.username).prefix(1)).uppercased())
                .font(Theme.playing)
                .frame(width: 48, height: 48)
                .background(Circle().fill(Theme.tile))
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 0) {
                Text(user.displayName ?? user.username).font(Theme.bodyStrong)
                Text("\(user.username) · \(user.admin ? "Administrator" : "Member")").font(Theme.small).foregroundStyle(Theme.quiet)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var rows: some View {
        let shown = sections.filter { search.isEmpty || "\($0.title) \($0.detail)".localizedCaseInsensitiveContains(search) }
        return VStack(spacing: 0) {
            ForEach(shown, id: \.page) { section in
                NavigationLink(value: section.page) {
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(section.title).font(Theme.bodyMedium)
                            Text(section.detail).font(Theme.small).foregroundStyle(Theme.quiet)
                        }
                        Spacer(minLength: 8)
                        Icon("chevron-right", size: 16).foregroundStyle(Theme.quiet)
                    }
                    .frame(minHeight: 60)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .overlay(alignment: .bottom) { Theme.barLine.frame(height: 1) }
            }
            if shown.isEmpty {
                Text("No setting matches “\(search)”.").font(Theme.small).foregroundStyle(Theme.soft).padding(.vertical, 12)
            }
        }
    }

}

struct PrivacyView: View {
    @Environment(AppModel.self) private var model
    @State private var settings: PrivacySettings?
    @State private var failed = false
    @State private var problem: String?
    @State private var lockChange: LockChange?
    @State private var confirmingDelete = false
    @State private var notice: String?

    struct LockChange: Identifiable {
        let id = UUID()
        let pin: Bool
        let idleMinutes: Int
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                if let settings {
                    group("Screen lock") {
                        SettingRow(title: "PIN", detail: settings.pinEnabled ? "On" : "Off", divided: true) {
                            OutlineButton(title: settings.pinEnabled ? "Change" : "Set a PIN") {
                                lockChange = LockChange(pin: true, idleMinutes: settings.idleMinutes == 0 ? 15 : settings.idleMinutes)
                            }
                        }
                        SettingRow(title: "Lock when idle", detail: settings.pinEnabled ? nil : "Set a PIN first.") {
                            Menu {
                                ForEach(Privacy.idleChoices, id: \.self) { minutes in
                                    Button(idleLabel(minutes)) {
                                        guard minutes != settings.idleMinutes else { return }
                                        lockChange = LockChange(pin: false, idleMinutes: minutes)
                                    }
                                }
                            } label: {
                                HStack(spacing: 8) {
                                    Text(idleLabel(settings.idleMinutes)).font(Theme.bodyMedium)
                                    Icon("chevron-down", size: 16)
                                }
                                .foregroundStyle(Theme.text)
                                .padding(.leading, 16)
                                .padding(.trailing, 12)
                                .frame(minHeight: 44)
                                .overlay(Capsule().strokeBorder(Theme.line))
                            }
                            .disabled(!settings.pinEnabled)
                            .accessibilityLabel("Lock when idle: \(idleLabel(settings.idleMinutes))")
                        }
                    }
                    group("Listening history") {
                        SettingRow(title: "Save listening history", detail: "History and Recently played need it.", divided: true) {
                            Switch(on: settings.history, label: "Save listening history") { on in
                                await save { self.settings?.history = try await $0.saveHistory(on) }
                            }
                        }
                        SettingRow(title: "Delete listening history", detail: "Removes every entry.") {
                            Button { confirmingDelete = true } label: {
                                Text("Delete…").font(Theme.smallMedium).foregroundStyle(Color(hex: 0xf0a08c)).padding(.horizontal, 12).frame(minHeight: 44)
                            }
                            .buttonStyle(.plain)
                            .confirmationDialog("Delete your listening history?", isPresented: $confirmingDelete, titleVisibility: .visible) {
                                Button("Delete history", role: .destructive) {
                                    Task { await save { try await $0.deleteHistory(); notice = "Listening history deleted." } }
                                }
                            } message: {
                                Text("Every entry is removed. History and Recently played start again from nothing. This can't be undone.")
                            }
                            .padding(.trailing, -12)
                        }
                    }
                    group("What Hito sends out") {
                        SettingRow(title: "Look up artists on MusicBrainz", detail: "Sends track codes, artist links and sometimes artist names.", divided: true) {
                            Switch(on: settings.artistLookup, label: "Look up artists on MusicBrainz") { on in
                                await save { self.settings?.artistLookup = try await $0.saveLookup(.artistLookup, on).artistLookup }
                            }
                        }
                        SettingRow(title: "Fetch album covers", detail: "Uses only a MusicBrainz identifier.") {
                            Switch(on: settings.artwork, label: "Fetch album covers") { on in
                                await save { self.settings?.artwork = try await $0.saveLookup(.artwork, on).artwork }
                            }
                        }
                    }
                    if let problem {
                        Text(problem).font(Theme.small).foregroundStyle(Theme.alert)
                    }
                    if let notice {
                        Text(notice).font(Theme.small).foregroundStyle(Theme.soft)
                    }
                } else if failed {
                    HStack(spacing: 8) {
                        Text("These settings didn't load.").foregroundStyle(Theme.soft)
                        TextAction(title: "Try again") { Task { await load() } }
                    }
                    .font(Theme.small)
                } else {
                    ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
        .task { if settings == nil { await load() } }
        .sheet(item: $lockChange) { change in
            LockChangeSheet(change: change) { pin, password in
                guard let api = model.api else { return nil }
                do {
                    let lock = try await Privacy(api: api).saveLock(pin: pin, idleMinutes: change.idleMinutes, password: password)
                    settings?.pinEnabled = lock.pinEnabled
                    settings?.idleMinutes = lock.idleMinutes
                    return nil
                } catch HitoAPI.Problem.refused {
                    return "That password didn't work, or the PIN isn't 4 to 8 digits."
                } catch {
                    model.handle(error)
                    return "This didn't save. Try again."
                }
            }
        }
    }

    private func idleLabel(_ minutes: Int) -> String {
        switch minutes {
        case 0: "Never"
        case 60: "1 hour"
        default: "\(minutes) minutes"
        }
    }

    private func group(_ title: String, @ViewBuilder rows: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(Theme.section).frame(minHeight: 44).accessibilityAddTraits(.isHeader)
            VStack(spacing: 0) { rows() }
        }
    }

    private func save(_ change: (Privacy) async throws -> Void) async {
        guard let api = model.api else { return }
        problem = nil
        notice = nil
        do {
            try await change(Privacy(api: api))
        } catch {
            problem = "This didn't save. Check your connection and try again."
            model.handle(error)
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        failed = false
        do {
            settings = try await Privacy(api: api).load()
        } catch {
            failed = true
            model.handle(error)
        }
    }
}

struct SettingRow<Control: View>: View {
    let title: String
    var detail: String?
    var divided = false
    @ViewBuilder let control: Control

    var body: some View {
        HStack(spacing: 16) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(Theme.bodyMedium)
                if let detail { Text(detail).font(Theme.small).foregroundStyle(Theme.quiet) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            control
        }
        .padding(.vertical, 10)
        .frame(minHeight: 64)
        .overlay(alignment: .bottom) { if divided { Theme.barLine.frame(height: 1) } }
    }
}

struct OutlineButton: View {
    let title: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(Theme.bodyMedium)
                .foregroundStyle(Theme.text)
                .padding(.horizontal, 18)
                .frame(minHeight: 44)
                .overlay(Capsule().strokeBorder(Theme.line))
        }
        .buttonStyle(.plain)
    }
}

struct Switch: View {
    let on: Bool
    let label: String
    let change: (Bool) async -> Void
    @State private var saving = false

    var body: some View {
        Button {
            Task {
                saving = true
                await change(!on)
                saving = false
            }
        } label: {
            ZStack(alignment: on ? .trailing : .leading) {
                Capsule().fill(on ? Theme.accent : Theme.line)
                Circle().fill(on ? Theme.onAccent : Theme.soft).padding(3)
            }
            .frame(width: 44, height: 26)
            .frame(minHeight: 44)
            .opacity(saving ? 0.6 : 1)
            .animation(.snappy(duration: 0.18), value: on)
        }
        .buttonStyle(.plain)
        .disabled(saving)
        .accessibilityLabel(label)
        .accessibilityValue(on ? "On" : "Off")
        .accessibilityAddTraits(.isToggle)
    }
}

struct LockChangeSheet: View {
    @Environment(\.dismiss) private var dismiss
    let change: PrivacyView.LockChange
    let save: (String?, String) async -> String?
    @State private var pin = ""
    @State private var password = ""
    @State private var problem: String?
    @State private var saving = false

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(change.pin ? "Choose a PIN" : "Confirm it's you").font(Theme.playing).accessibilityAddTraits(.isHeader)
            Text(change.pin ? "Use 4 to 8 digits. The screen locks after a while without use, and the PIN opens it." : "Enter your password to change when the screen locks.")
                .font(Theme.small)
                .foregroundStyle(Theme.soft)
            if change.pin {
                SecureField("", text: $pin, prompt: Text("New PIN").foregroundStyle(Theme.quiet))
                    .keyboardType(.numberPad)
                    .textContentType(.oneTimeCode)
                    .accessibilityLabel("New PIN")
                    .padding(.horizontal, 16)
                    .frame(minHeight: 48)
                    .background(Capsule().fill(Theme.ground))
            }
            SecureField("", text: $password, prompt: Text("Your password").foregroundStyle(Theme.quiet))
                .textContentType(.password)
                .accessibilityLabel("Your password")
                .padding(.horizontal, 16)
                .frame(minHeight: 48)
                .background(Capsule().fill(Theme.ground))
            if let problem {
                Text(problem).font(Theme.small).foregroundStyle(Theme.alert)
            }
            Button {
                Task {
                    saving = true
                    problem = await save(change.pin ? pin : nil, password)
                    saving = false
                    if problem == nil { dismiss() }
                }
            } label: {
                Text(saving ? "Saving…" : "Save")
                    .font(Theme.bodyStrong)
                    .foregroundStyle(Theme.onAccent)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .background(Capsule().fill(Theme.accent))
            }
            .buttonStyle(.plain)
            .disabled(saving || password.isEmpty || (change.pin && pin.isEmpty))
            Spacer(minLength: 0)
        }
        .padding(20)
        .foregroundStyle(Theme.text)
        .presentationDetents([.medium])
        .presentationBackground(Theme.field)
    }
}
