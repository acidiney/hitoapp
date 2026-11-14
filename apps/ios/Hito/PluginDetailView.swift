import HitoKit
import SwiftUI

struct PluginDetailView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @Environment(\.dynamicTypeSize) private var typeSize
    @State private var connection: PluginConnection
    @State private var review: (digest: String, wants: [String])?
    @State private var working = false
    @State private var note: String?
    @State private var problem: String?
    @State private var confirmingRemove = false
    @State private var confirmingReview = false
    @State private var account: PluginAccount?
    @State private var accountUnknown = false
    @State private var confirmingSignOut = false

    init(connection: PluginConnection) {
        _connection = State(initialValue: connection)
    }

    private static let what: [String: String] = [
        "Search": "Finds music beyond your library, only when you ask.",
        "Playback": "Plays tracks you have no file for, after local files and saved copies.",
        "Track details": "Fills in missing titles, artists and albums. Your own edits always win.",
        "Playlists": "Follows or copies a playlist from a link.",
        "Covers": "Fetches album artwork.",
        "Lyrics": "Shows synced lyrics in now playing.",
        "Links": "Looks up the song behind a link you paste on a track. You check the match before it becomes a source.",
        "Browse": "Lists albums and artists from the service.",
        "Faster playback": "Gets songs ready sooner.",
        "Featured": "Suggests playlists on Discover.",
    ]

    var capabilities: [String] {
        connection.summary.isEmpty ? [] : connection.summary.components(separatedBy: " · ")
    }

    var body: some View {
        ScrollView {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 14) {
                PluginTile(name: connection.name, size: 56)
                VStack(alignment: .leading, spacing: 0) {
                    Text(connection.name).font(Theme.title).lineLimit(2).accessibilityAddTraits(.isHeader)
                    HStack(spacing: 8) {
                        Circle()
                            .fill(connection.health == .ok ? Theme.soft : .clear)
                            .strokeBorder(connection.health == .ok ? Theme.soft : Theme.warning, lineWidth: 1.5)
                            .frame(width: 9, height: 9)
                        Text(status).font(Theme.small).foregroundStyle(connection.health == .ok ? Theme.soft : Theme.warning)
                    }
                    .accessibilityElement(children: .combine)
                }
            }
            VStack(spacing: 0) {
                ForEach(Array(capabilities.enumerated()), id: \.element) { index, name in
                    VStack(alignment: .leading, spacing: 0) {
                        Text(name).font(Theme.bodyStrong)
                        Text(Self.what[name] ?? "").font(Theme.small).foregroundStyle(Theme.quiet)
                    }
                    .frame(maxWidth: .infinity, minHeight: 64, alignment: .leading)
                    .overlay(alignment: .top) { if index > 0 { Theme.barLine.frame(height: 1) } }
                    .accessibilityElement(children: .combine)
                }
                if connection.signsIn {
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 0) {
                            Text("Account sign-in").font(Theme.bodyStrong)
                            HStack(spacing: 6) {
                                Circle()
                                    .fill(account == .connected ? Theme.soft : .clear)
                                    .strokeBorder(account == .connected ? Theme.soft : Theme.warning, lineWidth: 1.5)
                                    .frame(width: 9, height: 9)
                                Text(accountLabel).font(Theme.small).foregroundStyle(account == .connected ? Theme.quiet : Theme.warning)
                            }
                        }
                        Spacer(minLength: 0)
                        if account == .connected {
                            Button { confirmingSignOut = true } label: {
                                Text("Sign out").font(Theme.smallStrong).foregroundStyle(Theme.text).padding(.horizontal, 16).frame(minHeight: 40).overlay(Capsule().strokeBorder(Theme.line))
                            }
                            .buttonStyle(.plain)
                            .disabled(working)
                            .confirmationDialog("Sign out of \(connection.name)?", isPresented: $confirmingSignOut, titleVisibility: .visible) {
                                Button("Sign out", role: .destructive) { Task { await signOut() } }
                            } message: {
                                Text("Hito stops using your \(connection.name) account. The plugin stays connected.")
                            }
                        } else if accountUnknown {
                            Button { Task { await loadAccount() } } label: {
                                Text("Check again").font(Theme.smallStrong).foregroundStyle(Theme.text).padding(.horizontal, 16).frame(minHeight: 40).overlay(Capsule().strokeBorder(Theme.line))
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .frame(minHeight: 64)
                    .overlay(alignment: .top) { if !capabilities.isEmpty { Theme.barLine.frame(height: 1) } }
                    .accessibilityElement(children: .contain)
                }
                if let review {
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 0) {
                            Text(review.wants.isEmpty ? "New permissions" : review.wants.joined(separator: ", ")).font(Theme.bodyStrong)
                            HStack(spacing: 6) {
                                Circle().strokeBorder(Theme.warning, lineWidth: 1.5).frame(width: 9, height: 9)
                                Text("Wants to add · off until you accept").font(Theme.smallStrong).foregroundStyle(Theme.warning)
                            }
                        }
                        Spacer(minLength: 0)
                        Button { confirmingReview = true } label: {
                            Text("Review").font(Theme.smallStrong).foregroundStyle(Theme.onAccent).padding(.horizontal, 18).frame(minHeight: 44).background(Capsule().fill(Theme.accent))
                        }
                        .buttonStyle(.plain)
                        .confirmationDialog("Let \(connection.name) add \(review.wants.isEmpty ? "new permissions" : review.wants.joined(separator: ", ").lowercased())?", isPresented: $confirmingReview, titleVisibility: .visible) {
                            Button("Accept") { Task { await check(accept: review.digest) } }
                        } message: {
                            Text("Hito starts using \(connection.name) for this as soon as you accept. You can disconnect the plugin at any time.")
                        }
                    }
                    .frame(minHeight: 64)
                    .overlay(alignment: .top) { Theme.barLine.frame(height: 1) }
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 4)
            .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
            if let problem {
                HStack(spacing: 10) {
                    Circle().strokeBorder(Color(hex: 0xf0a08c), style: StrokeStyle(lineWidth: 1.5, dash: [1.5, 1.5])).frame(width: 10, height: 10)
                    Text(problem).font(Theme.small).foregroundStyle(Color(hex: 0xf6d3ca))
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 10).fill(Color(hex: 0x2a1b17)))
            }
            if let note {
                Text(note).font(Theme.small).foregroundStyle(Theme.soft)
            }
            if typeSize.isAccessibilitySize {
                actions.padding(.top, 8)
            }
        }
        .padding(.horizontal, 16)
        .padding(.bottom, 16)
        }
        .scrollIndicators(.hidden)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if !typeSize.isAccessibilitySize {
                actions
                    .padding(.horizontal, 16)
                    .padding(.top, 8)
                    .padding(.bottom, 16)
                    .background(Theme.ground)
            }
        }
        .task(id: connection.signsIn) { await loadAccount() }
    }

    private var actions: some View {
        VStack(spacing: 8) {
            Button { Task { await check() } } label: {
                Text(working ? "Checking…" : "Check connection")
                    .font(Theme.bodyMedium)
                    .foregroundStyle(Theme.text)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .overlay(Capsule().strokeBorder(Theme.line))
            }
            .buttonStyle(.plain)
            .disabled(working)
            Button { confirmingRemove = true } label: {
                Text("Disconnect")
                    .font(Theme.bodyMedium)
                    .foregroundStyle(Theme.danger)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .overlay(Capsule().strokeBorder(Color(hex: 0x5a3a32)))
            }
            .buttonStyle(.plain)
            .disabled(working)
            .confirmationDialog("Disconnect \(connection.name)?", isPresented: $confirmingRemove, titleVisibility: .visible) {
                Button("Disconnect", role: .destructive) { Task { await remove() } }
            } message: {
                Text("Its saved token is cleared. Your library, playlists and saved copies stay. Tracks that only play from \(connection.name) will show No source.")
            }
            Text("Disconnecting leaves your library and playlists as they are.")
                .font(Theme.small)
                .foregroundStyle(Theme.quiet)
                .multilineTextAlignment(.center)
                .frame(maxWidth: .infinity)
        }
    }

    private var status: String {
        switch connection.health {
        case .ok:
            if let checked = connection.checkedAt {
                return "Connected · checked \(checked.formatted(.relative(presentation: .named)))"
            }
            return "Connected"
        case .degraded: return "Working, with problems"
        case .unreachable: return "Can't reach it right now"
        case .credentialsRequired: return "Needs you to sign in again on the web"
        }
    }

    private var accountLabel: String {
        if accountUnknown { return "Sign-in status unavailable" }
        switch account {
        case .connected: return "Connected"
        case .needsSignInAgain: return "Sign in again on the web"
        case .notConnected: return "Not connected · connect on the web"
        case nil: return "Checking sign-in…"
        }
    }

    private func loadAccount() async {
        guard connection.signsIn, let api = model.api else { return }
        accountUnknown = false
        do {
            account = try await Plugins(api: api).account(connection.id)
        } catch is CancellationError {
        } catch {
            accountUnknown = true
            model.handle(error)
        }
    }

    private func signOut() async {
        guard let api = model.api else { return }
        working = true
        defer { working = false }
        do {
            try await Plugins(api: api).signOut(connection.id)
            note = "Signed out of \(connection.name)."
            await loadAccount()
        } catch {
            problem = "You're still signed in to \(connection.name). Try again."
            model.handle(error)
        }
    }

    private func check(accept digest: String? = nil) async {
        guard let api = model.api else { return }
        working = true
        problem = nil
        note = nil
        defer { working = false }
        do {
            switch try await Plugins(api: api).check(connection.id, revision: connection.revision, had: connection.capabilities, signsIn: connection.signsIn, accept: digest) {
            case let .updated(saved):
                review = nil
                connection = saved
                note = digest == nil ? "Connection checked." : "Accepted. \(connection.name) can use it now."
            case let .review(digest, wants):
                review = (digest, wants)
            }
        } catch HitoAPI.Problem.refused {
            problem = "\(connection.name) didn't answer. Check that its service is running, then try again."
        } catch {
            problem = "The check didn't finish. Try again."
            model.handle(error)
        }
    }

    private func remove() async {
        guard let api = model.api else { return }
        working = true
        defer { working = false }
        do {
            try await Plugins(api: api).remove(connection.id, revision: connection.revision)
            dismiss()
        } catch {
            problem = "\(connection.name) is still connected. Try again."
            model.handle(error)
        }
    }
}
