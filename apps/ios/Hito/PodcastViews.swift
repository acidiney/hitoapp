import HitoKit
import SwiftUI

struct PodcastsSection: View {
    @Environment(AppModel.self) private var model
    @State private var subscriptions: [Podcasts.Subscription]?
    @State private var feed = ""
    @State private var attempt: String?
    @State private var subscribing = false
    @State private var problem: String?
    @State private var loadProblem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 10) {
                Text("Subscribe to a podcast").font(Theme.section).accessibilityAddTraits(.isHeader)
                Text("Subscribe with a feed address to follow a show. New episodes appear on its page after a feed check.")
                    .font(Theme.small).foregroundStyle(Theme.soft)
                HStack(spacing: 8) {
                    TextField("", text: $feed, prompt: Text("Feed address").foregroundStyle(Theme.quiet))
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .submitLabel(.go)
                        .onSubmit { Task { await subscribe() } }
                        .onChange(of: feed) { attempt = nil }
                        .padding(.horizontal, 14)
                        .frame(minHeight: 44)
                        .background(Capsule().fill(Theme.raised))
                        .accessibilityLabel("Feed address")
                    Button(subscribing ? "Subscribing…" : attempt == nil ? "Subscribe" : "Retry subscription") { Task { await subscribe() } }
                        .buttonStyle(.plain)
                        .font(Theme.bodyStrong)
                        .foregroundStyle(Theme.onAccent)
                        .padding(.horizontal, 16)
                        .frame(minHeight: 44)
                        .background(Capsule().fill(Theme.accent))
                        .disabled(subscribing || feed.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.warning) }
            }
            .padding(14)
            .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
            if let loadProblem { Text(loadProblem).font(Theme.small).foregroundStyle(Theme.warning) }
            if let subscriptions {
                if subscriptions.isEmpty {
                    Text("No podcast subscriptions yet").font(Theme.bodyStrong)
                } else {
                    Text("Subscribed podcasts").font(Theme.section).accessibilityAddTraits(.isHeader)
                    ForEach(subscriptions) { subscription in
                        NavigationLink(value: subscription) {
                            HStack(spacing: 12) {
                                ZStack { Theme.tile; Icon("podcast", size: 20).foregroundStyle(Theme.coverMark) }
                                    .frame(width: 44, height: 44)
                                    .clipShape(RoundedRectangle(cornerRadius: 6))
                                VStack(alignment: .leading, spacing: 0) {
                                    Text(subscription.name).lineLimit(1)
                                    Text(subscription.status ?? "Podcast").font(Theme.small).foregroundStyle(subscription.refreshFailed ? Theme.warning : Theme.quiet)
                                }
                                Spacer(minLength: 8)
                                Icon("chevron-right", size: 16).foregroundStyle(Theme.quiet)
                            }
                            .frame(minHeight: 60)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
            } else if loadProblem == nil {
                ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
            }
        }
        .task { await load() }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            subscriptions = try await Podcasts(api: api).subscriptions()
            loadProblem = nil
        } catch {
            loadProblem = "Hito could not check podcast updates. Your last checked subscriptions are still shown."
        }
    }

    private func subscribe() async {
        guard let api = model.api, !subscribing else { return }
        let address = feed.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !address.isEmpty else { return }
        guard address.utf8.count <= 2048 else {
            problem = "Use a feed address of up to 2,048 bytes."
            return
        }
        let request = attempt ?? UUID().uuidString.lowercased()
        attempt = request
        subscribing = true
        defer { subscribing = false }
        do {
            _ = try await Podcasts(api: api).subscribe(feed: address, request: request)
            attempt = nil
            feed = ""
            problem = nil
            await load()
        } catch Podcasts.Problem.rejected {
            attempt = nil
            problem = "Hito rejected the subscription request. Check the feed address and try again."
        } catch {
            problem = "Hito could not confirm the subscription. Retry the same request to recover its result."
        }
    }
}

struct PodcastView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let subscription: Podcasts.Subscription
    @State private var current: Podcasts.Subscription?
    @State private var page: Podcasts.EpisodePage?
    @State private var more: [Podcasts.Episode] = []
    @State private var next: String?
    @State private var unplayedOnly = true
    @State private var problem: String?
    @State private var refreshing = false
    @State private var unsubscribing = false
    @State private var confirmingUnsubscribe = false

    var shown: Podcasts.Subscription { current ?? subscription }
    var episodes: [Podcasts.Episode] { (page?.episodes ?? []) + more }
    var visible: [Podcasts.Episode] { unplayedOnly ? episodes.filter { $0.state != .played } : episodes }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                header
                if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.warning) }
                HStack {
                    Text("Episodes").font(Theme.section).accessibilityAddTraits(.isHeader)
                    Spacer()
                    Picker("Episode filter", selection: $unplayedOnly) {
                        Text("Unplayed").tag(true)
                        Text("All").tag(false)
                    }
                    .pickerStyle(.segmented)
                    .frame(width: 180)
                }
                if page == nil, problem == nil {
                    Text("Loading episodes and saved positions…").font(Theme.small).foregroundStyle(Theme.soft)
                } else if episodes.isEmpty {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("No episodes yet").font(Theme.bodyStrong)
                        Text("Episodes appear here after a successful feed check.").font(Theme.small).foregroundStyle(Theme.soft)
                    }
                } else if visible.isEmpty {
                    Text("You have played every episode on this page.").font(Theme.small).foregroundStyle(Theme.soft)
                    TextAction(title: "Show all episodes") { unplayedOnly = false }
                }
                VStack(spacing: 0) {
                    ForEach(visible) { episode in
                        EpisodeRow(episode: episode, isCurrent: model.player?.current?.id == episode.track.id) { fromStart in
                            play(episode, fromStart: fromStart)
                        }
                    }
                }
                if next != nil {
                    TextAction(title: "Load more episodes") { Task { await loadMore() } }
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
        .task { await load() }
        .refreshable { await load() }
        .confirmationDialog("Unsubscribe from \(shown.name)?", isPresented: $confirmingUnsubscribe, titleVisibility: .visible) {
            Button("Unsubscribe", role: .destructive) { Task { await unsubscribe() } }
        } message: {
            Text("The podcast leaves your library. Your saved positions stay with the episodes.")
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .center, spacing: 14) {
                Cover(path: episodes.first?.track.artworkPath ?? ["api", "v1", "podcasts", subscription.id, "artwork"], size: 96, radius: 10, mark: "podcast")
                VStack(alignment: .leading, spacing: 4) {
                    Text("Podcast").font(Theme.smallStrong).foregroundStyle(Theme.soft)
                    Text(shown.name).font(Theme.title).tracking(Theme.titleTracking).lineLimit(3).accessibilityAddTraits(.isHeader)
                    if let page { Text("\(page.total) \(page.total == 1 ? "episode" : "episodes")").foregroundStyle(Theme.soft) }
                    if let status = shown.status {
                        Text(status).font(Theme.small).foregroundStyle(shown.refreshFailed ? Theme.warning : Theme.quiet)
                    }
                }
            }
            HStack(spacing: 8) {
                if let page, let lead = page.lead, let label = page.leadLabel {
                    PlayAllButton(label: label) { play(lead, fromStart: false) }
                }
                Menu {
                    Button(refreshing ? "Requesting refresh…" : "Refresh feed") { Task { await refresh() } }
                        .disabled(refreshing || shown.refreshPending)
                    Button(unsubscribing ? "Unsubscribing…" : "Unsubscribe", role: .destructive) { confirmingUnsubscribe = true }
                        .disabled(unsubscribing)
                } label: {
                    Icon("more").foregroundStyle(Theme.text).frame(width: 48, height: 48).contentShape(Circle())
                }
                .accessibilityLabel("Podcast controls")
            }
        }
        .padding(.top, 8)
    }

    private func play(_ episode: Podcasts.Episode, fromStart: Bool) {
        guard let index = episodes.firstIndex(of: episode) else { return }
        let start = fromStart || episode.state == .played ? 0 : Double(episode.positionMs) / 1000
        model.player?.play(episodes.map(\.track), at: index, from: shown.name, startingAt: start)
    }

    private func load() async {
        guard let api = model.api else { return }
        let podcasts = Podcasts(api: api)
        do {
            async let status = podcasts.subscriptions()
            async let first = podcasts.episodes(subscription.id, after: nil)
            let (all, loaded) = try await (status, first)
            current = all.first { $0.id == subscription.id } ?? current
            page = loaded
            more = []
            next = loaded.next
            problem = nil
        } catch {
            problem = "Hito could not load these episodes and their saved positions. The previously shown episodes are still available."
        }
    }

    private func loadMore() async {
        guard let api = model.api, let after = next else { return }
        do {
            let loaded = try await Podcasts(api: api).episodes(subscription.id, after: after)
            more += loaded.episodes
            next = loaded.next
        } catch {
            problem = "Hito could not load these episodes and their saved positions. The previously shown episodes are still available."
        }
    }

    private func refresh() async {
        guard let api = model.api else { return }
        refreshing = true
        defer { refreshing = false }
        do {
            try await Podcasts(api: api).refresh(subscription.id, request: UUID().uuidString.lowercased())
            problem = nil
            for _ in 0..<30 {
                try? await Task.sleep(for: .seconds(2))
                guard !Task.isCancelled else { return }
                let all = (try? await Podcasts(api: api).subscriptions()) ?? []
                current = all.first { $0.id == subscription.id } ?? current
                if current?.refreshPending == false { break }
            }
            if current?.refreshPending == true {
                problem = "This feed check is taking longer than expected. Check again for an update."
            }
            await load()
        } catch {
            problem = "Hito could not check this feed. Previously shown episodes remain available."
        }
    }

    private func unsubscribe() async {
        guard let api = model.api else { return }
        unsubscribing = true
        defer { unsubscribing = false }
        do {
            try await Podcasts(api: api).unsubscribe(subscription.id, request: UUID().uuidString.lowercased())
            dismiss()
        } catch {
            problem = "Your subscription request was not confirmed."
        }
    }
}

struct EpisodeRow: View {
    let episode: Podcasts.Episode
    let isCurrent: Bool
    let play: (Bool) -> Void

    var body: some View {
        HStack(spacing: 8) {
            Button { play(false) } label: {
                VStack(alignment: .leading, spacing: 4) {
                    Text(episode.track.title)
                        .font(Theme.bodyMedium)
                        .foregroundStyle(isCurrent ? Theme.focus : Theme.text)
                        .lineLimit(2)
                    Text(episode.state.label).font(Theme.small).foregroundStyle(Theme.quiet)
                    if case let .remaining(_, fraction) = episode.state {
                        GeometryReader { proxy in
                            ZStack(alignment: .leading) {
                                Capsule().fill(Theme.rest)
                                Capsule().fill(Theme.accent).frame(width: proxy.size.width * fraction)
                            }
                        }
                        .frame(height: 3)
                        .accessibilityHidden(true)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 10)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if case .remaining = episode.state {
                Button { play(true) } label: {
                    Image(systemName: "arrow.counterclockwise").foregroundStyle(Theme.quiet).frame(width: 44, height: 44)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Start over \(episode.track.title)")
            }
            Menu { TrackMenu(track: episode.track) } label: {
                Icon("more-vertical").foregroundStyle(Theme.quiet).frame(width: 44, height: 44).contentShape(Circle())
            }
            .accessibilityLabel("More actions for \(episode.track.title)")
        }
    }
}
