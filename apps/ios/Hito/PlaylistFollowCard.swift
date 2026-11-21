import HitoKit
import SwiftUI

struct PlaylistFollowCard: View {
    @Environment(AppModel.self) private var model
    let playlist: String
    let changed: () async -> Void
    @State private var follow: PlaylistFollowing.Follow?
    @State private var statusProblem: String?
    @State private var refreshProblem: String?
    @State private var resumeProblem: String?
    @State private var attempt: String?
    @State private var refreshing = false
    @State private var resuming = false
    @State private var waiting = false
    @State private var reconnecting: PluginOffer?
    @State private var finding = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Playlist updates").font(Theme.section).accessibilityAddTraits(.isHeader)
            if let follow {
                HStack(spacing: 8) {
                    Circle().fill(tone(follow.state)).frame(width: 8, height: 8).accessibilityHidden(true)
                    Text(follow.state.label).font(Theme.smallStrong)
                    Text("· \(follow.intervalLabel)").font(Theme.small).foregroundStyle(Theme.quiet)
                }
                .accessibilityElement(children: .combine)
                HStack(spacing: 20) {
                    time("Last checked", follow.lastChecked)
                    time("Next check", follow.nextCheck)
                }
                if follow.state == .failed {
                    Text("The last update failed. Your saved entries remain available.").font(Theme.small).foregroundStyle(Theme.warning)
                }
                if follow.state == .paused { paused(follow) }
            } else if statusProblem == nil {
                Text("Checking playlist updates…").font(Theme.small).foregroundStyle(Theme.soft)
            }
            Text("Read-only playlist. Its provider updates its name, tracks and order. Tracks need an available source to play.")
                .font(Theme.small).foregroundStyle(Theme.quiet)
            ForEach([statusProblem, refreshProblem, resumeProblem].compactMap { $0 }, id: \.self) { problem in
                Text(problem).font(Theme.small).foregroundStyle(Theme.warning)
            }
            HStack(spacing: 8) {
                if statusProblem != nil {
                    plain("Retry status check") { Task { await load() } }
                } else if attempt != nil {
                    plain(refreshing ? "Asking…" : "Retry refresh request") { Task { await refresh() } }
                        .disabled(refreshing || follow?.state == .paused)
                } else {
                    plain(refreshing ? "Asking…" : "Refresh now") { Task { await refresh() } }
                        .disabled(follow == nil || refreshing || waiting || follow?.busy == true || follow?.state == .paused)
                        .opacity(follow == nil || follow?.busy == true || follow?.state == .paused ? 0.5 : 1)
                }
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
        .task(id: playlist) { await watch() }
        .fullScreenCover(item: $reconnecting) { offer in
            ConnectSheet(offer: offer) { Task { await load() } }
                .presentationBackground(.clear)
        }
    }

    @ViewBuilder private func paused(_ follow: PlaylistFollowing.Follow) -> some View {
        if let review = follow.review {
            Text(review.sentence).font(Theme.bodyStrong)
            ForEach(review.changes, id: \.kind) { change in
                VStack(alignment: .leading, spacing: 2) {
                    Text(change.label).font(Theme.caption).foregroundStyle(Theme.quiet)
                    Text("\(PlaylistFollowing.Change.value(change.before)) → \(PlaylistFollowing.Change.value(change.after))").font(Theme.small)
                }
                .accessibilityElement(children: .combine)
            }
        }
        if let review = follow.review, review.removed {
            Text(review.reconnectServiceId == nil ? "Ask your administrator to offer \(review.connectionName) again, then connect it." : review.reconnectNote)
                .font(Theme.small)
            if let service = review.reconnectServiceId {
                PrimaryButton(title: review.reconnectTitle, busyTitle: "Opening…", busy: finding) { Task { await reconnect(service) } }
                    .disabled(finding)
            }
        }
        Text("Your saved entries remain available.").font(Theme.small).foregroundStyle(Theme.soft)
        if follow.review?.resumable == true {
            PrimaryButton(title: "Resume updates", busyTitle: "Resuming…", busy: resuming) { Task { await resume(follow) } }
                .disabled(resuming)
        }
    }

    private func plain(_ title: String, action: @escaping () -> Void) -> some View {
        Button(title, action: action)
            .buttonStyle(.plain)
            .font(Theme.bodyMedium)
            .padding(.horizontal, 18)
            .frame(minHeight: 44)
            .overlay(Capsule().strokeBorder(Theme.line))
    }

    private func time(_ label: String, _ date: Date) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label).font(Theme.caption).foregroundStyle(Theme.quiet)
            Text(Self.friendly(date)).font(Theme.small)
        }
        .accessibilityElement(children: .combine)
    }

    private func tone(_ state: PlaylistFollowing.State) -> Color {
        switch state {
        case .ready: Theme.soft
        case .queued, .running: Theme.focus
        case .failed, .paused: Theme.warning
        }
    }

    static func friendly(_ date: Date, now: Date = .now) -> String {
        let calendar = Calendar.current
        let clock = date.formatted(date: .omitted, time: .shortened)
        if calendar.isDateInToday(date) { return "Today, \(clock)" }
        if calendar.isDateInTomorrow(date) { return "Tomorrow, \(clock)" }
        if calendar.isDateInYesterday(date) { return "Yesterday, \(clock)" }
        let sameYear = calendar.component(.year, from: date) == calendar.component(.year, from: now)
        let day = sameYear ? date.formatted(.dateTime.day().month(.abbreviated)) : date.formatted(.dateTime.day().month(.abbreviated).year())
        return "\(day), \(clock)"
    }

    private func watch() async {
        while !Task.isCancelled {
            await load()
            let pause: Duration = statusProblem == nil && follow?.busy == true ? .seconds(1) : .seconds(30)
            try? await Task.sleep(for: pause)
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            let next = try await PlaylistFollowing(api: api).status(playlist)
            let previous = follow
            follow = next
            statusProblem = nil
            let finished = waiting && !next.busy
            if !next.busy { waiting = false }
            if finished || (previous.map { $0.contentRevision != next.contentRevision } ?? false) {
                await changed()
            }
        } catch {
            if !Task.isCancelled { statusProblem = "Hito couldn't check this playlist. Try again." }
        }
    }

    private func refresh() async {
        guard let api = model.api else { return }
        let request = attempt ?? UUID().uuidString.lowercased()
        attempt = request
        refreshing = true
        defer { refreshing = false }
        do {
            try await PlaylistFollowing(api: api).refresh(playlist, request: request)
            attempt = nil
            refreshProblem = nil
            waiting = true
        } catch PlaylistFollowing.Problem.busy {
            attempt = nil
            refreshProblem = nil
            waiting = true
        } catch {
            refreshProblem = "Hito couldn't ask for an update. Try again."
        }
        await load()
    }

    private func reconnect(_ service: String) async {
        guard let api = model.api else { return }
        finding = true
        defer { finding = false }
        do {
            let offer = try await Plugins(api: api).load().available.first { $0.id == service }
            reconnecting = offer
            resumeProblem = offer == nil ? "This plugin isn't offered to connect right now. Try again later." : nil
        } catch {
            resumeProblem = "Hito couldn't open the plugin. Try again."
        }
    }

    private func resume(_ current: PlaylistFollowing.Follow) async {
        guard let api = model.api else { return }
        resuming = true
        defer { resuming = false }
        do {
            try await PlaylistFollowing(api: api).resume(playlist, current, device: Player.device)
            resumeProblem = nil
        } catch {
            resumeProblem = "Updates didn't resume. Try again."
        }
        await load()
    }
}
