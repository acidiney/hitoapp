import HitoKit
import SwiftUI

struct MusicSettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var libraries: [ServerLibrary]?
    @State private var folders: [MusicFolder]?
    @State private var note: String?
    @State private var problem: String?
    @State private var adding = false
    @State private var removing: MusicFolder?
    @State private var confirmingMissing = false

    private var admin: Bool {
        if case let .signedIn(_, user) = model.step { return user.admin }
        return false
    }

    private var shared: ServerLibrary? { libraries?.first { !$0.personal } ?? libraries?.first }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                if admin, let shared {
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text("Music folders").font(Theme.section).accessibilityAddTraits(.isHeader)
                            Spacer()
                            OutlineButton(title: "Add folder") { adding = true }
                        }
                        Text("Your originals stay exactly where they are.").font(Theme.small).foregroundStyle(Theme.quiet)
                        if let folders {
                            if folders.isEmpty {
                                Text("No folders yet. Add one the server can read.").font(Theme.small).foregroundStyle(Theme.soft).padding(.vertical, 10)
                            }
                            ForEach(folders) { folder in
                                HStack(spacing: 14) {
                                    ZStack { Theme.tile; Icon("folder", size: 20).foregroundStyle(Theme.quiet) }
                                        .frame(width: 44, height: 44)
                                        .clipShape(RoundedRectangle(cornerRadius: 8))
                                        .accessibilityHidden(true)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(folder.name).font(Theme.bodyMedium).lineLimit(1)
                                        Text(folder.status).font(Theme.small).foregroundStyle(folder.available ? Theme.quiet : Theme.warning)
                                    }
                                    Spacer(minLength: 8)
                                    TextAction(title: "Scan") { Task { await scan(folder) } }
                                    Menu {
                                        Button("Remove folder…", role: .destructive) { removing = folder }
                                    } label: {
                                        Icon("more", size: 18).foregroundStyle(Theme.text).frame(width: 44, height: 44)
                                    }
                                    .accessibilityLabel("More for \(folder.name)")
                                }
                                .frame(minHeight: 64)
                                .overlay(alignment: .bottom) { Theme.barLine.frame(height: 1) }
                                .confirmationDialog("Remove \(folder.name)?", isPresented: Binding(get: { removing?.id == folder.id }, set: { if !$0 { removing = nil } }), titleVisibility: .visible) {
                                    Button("Remove folder", role: .destructive) { Task { await remove(folder) } }
                                } message: {
                                    Text("Hito stops reading it. The files stay where they are. Its tracks show as missing until you add it again.")
                                }
                            }
                        } else {
                            ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
                        }
                        SettingRow(title: "Scan all folders", detail: "Finds new files and changed tags.", divided: true) {
                            OutlineButton(title: "Scan all") { Task { await scanAll(shared) } }
                        }
                        SettingRow(title: "Tracks with missing files", detail: "Kept for 30 days after their files disappear, then removed. Tracks in playlists, likes or downloads are kept.", divided: true) {
                            Button { confirmingMissing = true } label: {
                                Text("Remove now…").font(Theme.smallMedium).foregroundStyle(Color(hex: 0xf0a08c)).frame(minHeight: 44)
                            }
                            .buttonStyle(.plain)
                            .confirmationDialog("Remove tracks with missing files?", isPresented: $confirmingMissing, titleVisibility: .visible) {
                                Button("Remove now", role: .destructive) { Task { await removeMissing(shared) } }
                            } message: {
                                Text("Tracks whose files are gone are removed from the library now. Tracks in playlists, likes or downloads stay.")
                            }
                        }
                        Text("Folder on another drive? Share it in Hito Setup on the server computer, then add it here.")
                            .font(Theme.small).foregroundStyle(Theme.quiet).padding(.top, 10)
                    }
                }
                if let note { Text(note).font(Theme.small).foregroundStyle(Theme.soft) }
                if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.alert) }
                VStack(alignment: .leading, spacing: 4) {
                    Text("Libraries you can listen to").font(Theme.section).accessibilityAddTraits(.isHeader)
                    if let libraries {
                        if libraries.isEmpty {
                            Text("None yet. An administrator gives you access.").font(Theme.small).foregroundStyle(Theme.soft).padding(.vertical, 10)
                        }
                        ForEach(libraries) { library in
                            HStack(spacing: 14) {
                                ZStack { Theme.tile; Icon("library", size: 20).foregroundStyle(Theme.quiet) }
                                    .frame(width: 44, height: 44)
                                    .clipShape(RoundedRectangle(cornerRadius: 8))
                                    .accessibilityHidden(true)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(library.name).font(Theme.bodyMedium)
                                    Text(library.personal ? "Only you · read-only" : "Shared on this server · read-only").font(Theme.small).foregroundStyle(Theme.quiet)
                                }
                                Spacer()
                            }
                            .frame(minHeight: 64)
                            .overlay(alignment: .bottom) { Theme.barLine.frame(height: 1) }
                            .accessibilityElement(children: .combine)
                        }
                    } else {
                        ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
        .refreshable { await load() }
        .task { await load() }
        .task(id: folders?.contains { $0.scanning != nil } ?? false) {
            while folders?.contains(where: { $0.scanning != nil }) == true {
                try? await Task.sleep(for: .seconds(2))
                guard !Task.isCancelled, let api = model.api, let shared else { return }
                if let fresh = try? await Admin(api: api).folders(shared.id) { folders = fresh }
            }
        }
        .sheet(isPresented: $adding) {
            if let shared {
                FolderPicker(library: shared) { name in
                    note = "Added \(name). Scanning it now."
                    Task { await load() }
                }
            }
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        let admin = Admin(api: api)
        do {
            let found = try await admin.libraries()
            libraries = found
            if self.admin, let shared = found.first(where: { !$0.personal }) ?? found.first {
                folders = try await admin.folders(shared.id)
            }
        } catch {
            libraries = libraries ?? []
            problem = "Music settings didn't load. Pull to try again."
            model.handle(error)
        }
    }

    private func act(_ success: String, _ work: (Admin) async throws -> Void) async {
        guard let api = model.api else { return }
        problem = nil
        do {
            try await work(Admin(api: api))
            note = success
            await load()
        } catch {
            problem = "That didn't work. Try again."
            model.handle(error)
        }
    }

    private func scan(_ folder: MusicFolder) async {
        await act("Scanning \(folder.name).") { try await $0.scan(folder: folder.id) }
    }

    private func scanAll(_ library: ServerLibrary) async {
        await act("Scanning every folder.") { try await $0.scanAll(library.id) }
    }

    private func remove(_ folder: MusicFolder) async {
        await act("Removed \(folder.name). Its files are untouched.") { try await $0.removeFolder(folder.id, revision: folder.revision) }
    }

    private func removeMissing(_ library: ServerLibrary) async {
        guard let api = model.api else { return }
        problem = nil
        do {
            note = try await Admin(api: api).removeMissing(library.id)
        } catch {
            problem = "Nothing was removed. Try again."
            model.handle(error)
        }
    }
}

struct FolderPicker: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let library: ServerLibrary
    let added: (String) -> Void
    @State private var trail: [ServerDirectory] = []
    @State private var entries: [ServerDirectory]?
    @State private var problem: String?
    @State private var working = false

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                Text(trail.isEmpty ? "Folders the server shares" : trail.map(\.name).joined(separator: " / "))
                    .font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(2)
                ScrollView {
                    VStack(spacing: 0) {
                        if let entries {
                            if entries.isEmpty {
                                Text(trail.isEmpty ? "The server shares no folders yet. Share one in Hito Setup on the server computer." : "No folders inside.")
                                    .font(Theme.small).foregroundStyle(Theme.soft).padding(.vertical, 12)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            ForEach(entries) { entry in
                                Button { Task { await open(entry) } } label: {
                                    HStack(spacing: 12) {
                                        Icon("folder", size: 20).foregroundStyle(Theme.quiet)
                                        Text(entry.name).font(Theme.bodyMedium).lineLimit(1)
                                        Spacer()
                                        Icon("chevron-right", size: 14).foregroundStyle(Theme.quiet)
                                    }
                                    .frame(minHeight: 52)
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                                .overlay(alignment: .bottom) { Theme.barLine.frame(height: 1) }
                            }
                        } else {
                            ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 80)
                        }
                    }
                }
                if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.alert) }
                if let current = trail.last {
                    Button { Task { await add(current) } } label: {
                        Text(working ? "Adding…" : "Add \(current.name)")
                            .font(Theme.bodyStrong)
                            .foregroundStyle(Theme.onAccent)
                            .frame(maxWidth: .infinity, minHeight: 48)
                            .background(Capsule().fill(Theme.accent))
                    }
                    .buttonStyle(.plain)
                    .disabled(working)
                }
            }
            .padding(16)
            .foregroundStyle(Theme.text)
            .background(Theme.field.ignoresSafeArea())
            .navigationTitle("Add folder")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                if !trail.isEmpty {
                    ToolbarItem(placement: .navigation) {
                        Button("Up") { trail.removeLast(); Task { await reload() } }
                    }
                }
            }
        }
        .presentationBackground(Theme.field)
        .task { await reload() }
    }

    private func reload() async {
        guard let api = model.api else { return }
        entries = nil
        problem = nil
        do {
            if let last = trail.last {
                entries = try await Admin(api: api).browse(last.place)
            } else {
                entries = try await Admin(api: api).sharedDirectories()
            }
        } catch {
            entries = []
            problem = "The server's folders didn't load. Try again."
            model.handle(error)
        }
    }

    private func open(_ entry: ServerDirectory) async {
        trail.append(entry)
        await reload()
    }

    private func add(_ directory: ServerDirectory) async {
        guard let api = model.api else { return }
        working = true
        defer { working = false }
        do {
            try await Admin(api: api).addFolder(directory.place, to: library.id)
            added(directory.name)
            dismiss()
        } catch HitoAPI.Problem.refused(409) {
            problem = "\(directory.name) is already in the library, or inside a folder that is."
        } catch {
            problem = "\(directory.name) wasn't added. Try again."
            model.handle(error)
        }
    }
}

struct AccountsSettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var accounts: [ServerAccount]?
    @State private var library: ServerLibrary?
    @State private var note: String?
    @State private var problem: String?
    @State private var creating = false
    @State private var deleting: ServerAccount?

    private var me: String? {
        if case let .signedIn(_, user) = model.step { return user.id }
        return nil
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text("People").font(Theme.section).accessibilityAddTraits(.isHeader)
                    Spacer()
                    OutlineButton(title: "Create account") { creating = true }
                }
                Text("Each account has its own playlists, likes and listening history.").font(Theme.small).foregroundStyle(Theme.quiet)
                if let note { Text(note).font(Theme.small).foregroundStyle(Theme.soft).padding(.top, 8) }
                if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.alert).padding(.top, 8) }
                if let accounts {
                    ForEach(accounts) { account in
                        HStack(spacing: 14) {
                            Text(String(account.name.prefix(1)).uppercased())
                                .font(Theme.bodyStrong)
                                .frame(width: 44, height: 44)
                                .background(Circle().fill(Theme.tile))
                                .accessibilityHidden(true)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(account.name).font(Theme.bodyMedium)
                                Text(account.hint()).font(Theme.small).foregroundStyle(Theme.quiet).fixedSize(horizontal: false, vertical: true)
                            }
                            Spacer(minLength: 8)
                            if account.id == me {
                                Text("You").font(Theme.small).foregroundStyle(Theme.soft)
                            } else {
                                Menu {
                                    if !account.admin, let library {
                                        Button("Give access to \(library.name)") { Task { await access(account, library, true) } }
                                        Button("Remove access to \(library.name)") { Task { await access(account, library, false) } }
                                    }
                                    Button("Delete account…", role: .destructive) { deleting = account }
                                } label: {
                                    Icon("more", size: 18).foregroundStyle(Theme.text).frame(width: 44, height: 44)
                                }
                                .accessibilityLabel("More for \(account.name)")
                            }
                        }
                        .frame(minHeight: 72)
                        .overlay(alignment: .bottom) { Theme.barLine.frame(height: 1) }
                    }
                } else {
                    ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
        .refreshable { await load() }
        .task { await load() }
        .sheet(isPresented: $creating) {
            NewAccountSheet { username, password in
                guard let api = model.api else { return "Sign in again to create accounts." }
                do {
                    try await Admin(api: api).createAccount(username: username, password: password)
                    note = "Created \(username.trimmingCharacters(in: .whitespacesAndNewlines)). Give them access to a library so they can listen."
                    await load()
                    return nil
                } catch HitoAPI.Problem.refused(409) {
                    return "That username is taken."
                } catch HitoAPI.Problem.refused(422) {
                    return "Use a username without spaces and a password of at least 12 characters."
                } catch {
                    model.handle(error)
                    return "The account wasn't created. Try again."
                }
            }
        }
        .sheet(item: $deleting) { account in
            DeleteAccountSheet(account: account) {
                guard let api = model.api else { return "Sign in again to delete accounts." }
                do {
                    try await Admin(api: api).deleteAccount(account.id, username: account.username)
                    note = "Deleted \(account.name)."
                    await load()
                    return nil
                } catch {
                    model.handle(error)
                    return "\(account.name) wasn't deleted. Try again."
                }
            }
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        let admin = Admin(api: api)
        do {
            accounts = try await admin.accounts()
            library = try await admin.libraries().first { !$0.personal }
        } catch {
            accounts = accounts ?? []
            problem = "Accounts didn't load. Pull to try again."
            model.handle(error)
        }
    }

    private func access(_ account: ServerAccount, _ library: ServerLibrary, _ granted: Bool) async {
        guard let api = model.api else { return }
        problem = nil
        do {
            try await Admin(api: api).setAccess(library: library.id, account: account.id, to: granted)
            note = granted ? "\(account.name) can listen to \(library.name)." : "\(account.name) can't listen to \(library.name) anymore."
        } catch {
            problem = "Access didn't change. Try again."
            model.handle(error)
        }
    }
}

struct NewAccountSheet: View {
    @Environment(\.dismiss) private var dismiss
    let create: (String, String) async -> String?
    @State private var username = ""
    @State private var password = ""
    @State private var problem: String?
    @State private var saving = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Create account").font(Theme.playing).accessibilityAddTraits(.isHeader)
            Text("They sign in with these. Share the password privately; they can change it in Profile.")
                .font(Theme.small).foregroundStyle(Theme.soft)
            TextField("", text: $username, prompt: Text("Username").foregroundStyle(Theme.quiet))
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .textContentType(.username)
                .padding(.horizontal, 16)
                .frame(minHeight: 48)
                .background(Capsule().fill(Theme.ground))
            SecureField("", text: $password, prompt: Text("Password, at least 12 characters").foregroundStyle(Theme.quiet))
                .textContentType(.newPassword)
                .padding(.horizontal, 16)
                .frame(minHeight: 48)
                .background(Capsule().fill(Theme.ground))
            if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.alert) }
            Button {
                if password.count < 12 {
                    problem = "Use a password of at least 12 characters."
                    return
                }
                Task {
                    saving = true
                    problem = await create(username, password)
                    saving = false
                    if problem == nil { dismiss() }
                }
            } label: {
                Text(saving ? "Creating…" : "Create account")
                    .font(Theme.bodyStrong)
                    .foregroundStyle(Theme.onAccent)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .background(Capsule().fill(Theme.accent))
            }
            .buttonStyle(.plain)
            .disabled(saving || username.trimmingCharacters(in: .whitespaces).isEmpty || password.isEmpty)
            Spacer(minLength: 0)
        }
        .padding(20)
        .foregroundStyle(Theme.text)
        .presentationDetents([.large])
        .presentationBackground(Theme.field)
    }
}

struct DeleteAccountSheet: View {
    @Environment(\.dismiss) private var dismiss
    let account: ServerAccount
    let delete: () async -> String?
    @State private var typed = ""
    @State private var problem: String?
    @State private var working = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Delete \(account.name)?").font(Theme.playing).accessibilityAddTraits(.isHeader)
            Text("Their playlists, likes, history and connections are deleted. The music library stays. This can't be undone.")
                .font(Theme.small).foregroundStyle(Theme.soft)
            Text("Type \(account.username) to confirm.").font(Theme.small).foregroundStyle(Theme.quiet)
            TextField("", text: $typed)
                .accessibilityLabel("Type \(account.username) to confirm")
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .padding(.horizontal, 16)
                .frame(minHeight: 48)
                .background(Capsule().fill(Theme.ground))
            if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.alert) }
            Button {
                Task {
                    working = true
                    problem = await delete()
                    working = false
                    if problem == nil { dismiss() }
                }
            } label: {
                Text(working ? "Deleting…" : "Delete account")
                    .font(Theme.bodyStrong)
                    .foregroundStyle(Theme.danger)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .overlay(Capsule().strokeBorder(Color(hex: 0x5a3a32)))
            }
            .buttonStyle(.plain)
            .disabled(working || typed != account.username)
            Spacer(minLength: 0)
        }
        .padding(20)
        .foregroundStyle(Theme.text)
        .presentationDetents([.medium, .large])
        .presentationBackground(Theme.field)
    }
}
