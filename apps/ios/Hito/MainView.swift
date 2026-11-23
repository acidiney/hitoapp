import HitoKit
import OSLog
import SwiftUI

enum Tab: CaseIterable, Hashable {
    case home, search, discover, library, plugins

    var label: String {
        switch self {
        case .home: "Home"
        case .search: "Search"
        case .discover: "Discover"
        case .library: "Library"
        case .plugins: "Plugins"
        }
    }

    var icon: String {
        switch self {
        case .home: "home"
        case .search: "search"
        case .discover: "discover"
        case .library: "library"
        case .plugins: "plugins"
        }
    }
}

struct MainView: View {
    @Environment(AppModel.self) private var model
    let address: ServerAddress
    let user: User
    @State private var tab = Tab.home
    @State private var showingNowPlaying = false
    @State private var showingQueue = false
    @State private var typing = false
    @State private var visited: Set<Tab> = [.home]
    @State private var paths: [Tab: NavigationPath] = [:]
    @State private var servicesArtist: ArtistGroup?
    private static let log = Logger(subsystem: "app.hito", category: "player")

    var body: some View {
        VStack(spacing: 0) {
            ZStack {
                Theme.ground.ignoresSafeArea()
                ForEach(Tab.allCases.filter(visited.contains), id: \.self) { item in
                    screen(item)
                        .opacity(item == tab ? 1 : 0)
                        .allowsHitTesting(item == tab)
                        .accessibilityHidden(item != tab)
                }
            }
            .frame(maxHeight: .infinity)
            .scrollDismissesKeyboard(.immediately)
            if model.serverUnreachable {
                Unreachable()
            }
            if !typing {
                if let player = model.player, player.current != nil {
                    MiniPlayer(player: player, routes: routes(player), go: go) { showingNowPlaying = true }
                }
                TabBar(tab: $tab)
            }
        }
        .background(Theme.ground.ignoresSafeArea())
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in typing = true }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in typing = false }
        .foregroundStyle(Theme.text)
        .onChange(of: tab) { visited.insert(tab) }
        .task(id: model.player?.current?.id) { await findServicesArtist() }
        .fullScreenCover(isPresented: $showingNowPlaying) {
            if let player = model.player {
                NowPlayingView(player: player, routes: routes(player), openQueue: { showingQueue = true }, go: go)
                    .sheet(isPresented: $showingQueue) { QueueSheet(player: player) }
            }
        }
    }

    private func routes(_ player: Player) -> PlayerRoutes? {
        guard let track = player.current else { return nil }
        return PlayerRoutes(track: track, playlist: player.queue.playlist, identity: servicesArtist?.recordingId == track.id ? servicesArtist : nil)
    }

    private func go(_ route: PlayerRoute) {
        var path = paths[tab] ?? NavigationPath()
        switch route {
        case let .artist(artist): path.append(artist)
        case let .artistGroup(group): path.append(group)
        case let .album(album): path.append(album)
        case let .playlist(playlist): path.append(playlist)
        }
        paths[tab] = path
        Self.log.info("player.go_to kind=\(route.kind, privacy: .public)")
    }

    private func findServicesArtist() async {
        guard let track = model.player?.current, let api = model.api, PlayerRoutes(track: track).needsIdentity,
              servicesArtist?.recordingId != track.id else { return }
        let started = ContinuousClock.now
        var outcome = "unmatched"
        do {
            servicesArtist = try await ArtistIdentity(api: api).group(for: track)
            if servicesArtist != nil { outcome = "matched" }
        } catch {
            outcome = Task.isCancelled ? "cancelled" : "failed"
        }
        let elapsed = Int((ContinuousClock.now - started) / .milliseconds(1))
        Self.log.debug("player.artist_link outcome=\(outcome, privacy: .public) ms=\(elapsed, privacy: .public)")
    }

    private func path(_ item: Tab) -> Binding<NavigationPath> {
        Binding(get: { paths[item] ?? NavigationPath() }, set: { paths[item] = $0 })
    }
}

extension MainView {
    @ViewBuilder func screen(_ item: Tab) -> some View {
        switch item {
        case .home:
            NavigationStack(path: path(item)) { HomeView(user: user, openSearch: { tab = .search }, openDiscover: { tab = .discover }, openPlugins: { tab = .plugins }).browsing() }
        case .search:
            NavigationStack(path: path(item)) { SearchView(active: tab == .search).browsing() }
        case .discover:
            NavigationStack(path: path(item)) { DiscoverView(openSearch: { tab = .search }).browsing() }
        case .library:
            NavigationStack(path: path(item)) { LibraryView().browsing() }
        case .plugins:
            NavigationStack(path: path(item)) { PluginsView().browsing() }
        }
    }
}

extension View {
    func browsing() -> some View {
        navigationDestination(for: Album.self) { AlbumView(album: $0).pushed() }
            .navigationDestination(for: TrackList.self) { TrackListView(list: $0).pushed() }
            .navigationDestination(for: Track.self) { TrackView(track: $0).pushed() }
            .navigationDestination(for: Artist.self) { ArtistView(artist: $0).pushed() }
            .navigationDestination(for: Genre.self) { GenreView(genre: $0).pushed() }
            .navigationDestination(for: ArtistGroup.self) { ArtistGroupView(group: $0).pushed() }
            .navigationDestination(for: CatalogRelease.self) { CatalogAlbumView(route: $0).pushed() }
            .navigationDestination(for: Playlist.self) { PlaylistView(playlist: $0).pushed(tint: Theme.header) }
            .navigationDestination(for: Podcasts.Subscription.self) { PodcastView(subscription: $0).pushed() }
            .navigationDestination(for: AudiobookRoute.self) { AudiobookView(route: $0).pushed() }
            .navigationDestination(for: PluginConnection.self) { PluginDetailView(connection: $0).pushed(back: "Plugins") }
            .navigationDestination(for: SettingsPage.self) { page in
                switch page {
                case .root: SettingsView().pushed()
                case .profile: ProfileView().pushed(page.title)
                case .privacy: PrivacyView().pushed(page.title)
                case .playback: PlaybackSettingsView().pushed(page.title)
                case .music: MusicSettingsView().pushed(page.title)
                case .accounts: AccountsSettingsView().pushed(page.title)
                case .about: AboutView().pushed(page.title)
                }
            }
            .page()
    }

    func page() -> some View {
        toolbar(.hidden, for: .navigationBar)
            .background(Theme.ground.ignoresSafeArea())
            .foregroundStyle(Theme.text)
    }

    func pushed(_ title: String? = nil, back: String? = nil, tint: Color = Theme.ground) -> some View {
        modifier(Pushed(title: title, back: back, tint: tint))
    }
}

struct Pushed: ViewModifier {
    @Environment(\.dismiss) private var dismiss
    let title: String?
    let back: String?
    let tint: Color

    func body(content: Content) -> some View {
        content
            .safeAreaInset(edge: .top, spacing: 0) {
                HStack(spacing: 4) {
                    if let back {
                        Button { dismiss() } label: {
                            HStack(spacing: 8) {
                                Icon("chevron-left", size: 18)
                                Text(back).font(Theme.bodyStrong)
                            }
                            .foregroundStyle(Theme.focus)
                            .padding(.horizontal, 12)
                            .frame(minHeight: 44)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Back to \(back)")
                    } else {
                        IconButton(icon: "chevron-left", label: "Back", glyph: 22) { dismiss() }
                    }
                    if let title {
                        Text(title).font(Theme.pageTitle).lineLimit(1).accessibilityAddTraits(.isHeader)
                    }
                    Spacer()
                }
                .padding(.horizontal, 4)
                .padding(.bottom, title == nil ? 0 : 12)
                .background(tint.ignoresSafeArea(edges: .top))
            }
            .page()
    }
}

extension UINavigationController: @retroactive UIGestureRecognizerDelegate {
    override open func viewDidLoad() {
        super.viewDidLoad()
        interactivePopGestureRecognizer?.delegate = self
    }

    public func gestureRecognizerShouldBegin(_ recognizer: UIGestureRecognizer) -> Bool {
        viewControllers.count > 1
    }
}

struct TabBar: View {
    @Binding var tab: Tab

    var body: some View {
        HStack(spacing: 0) {
            ForEach(Tab.allCases, id: \.self) { item in
                let current = item == tab
                Button {
                    tab = item
                } label: {
                    VStack(spacing: 4) {
                        Icon(item.icon, size: 22)
                            .foregroundStyle(current ? Theme.accent : Theme.quiet)
                        Text(item.label).font(current ? Theme.tabCurrent : Theme.tab).lineLimit(1)
                    }
                    .foregroundStyle(current ? Theme.text : Theme.quiet)
                    .frame(maxWidth: .infinity, minHeight: 52)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(current ? .isSelected : [])
                .accessibilityShowsLargeContentViewer {
                    Icon(item.icon, size: 22)
                    Text(item.label)
                }
            }
        }
        .dynamicTypeSize(...DynamicTypeSize.large)
        .padding(.horizontal, 4)
        .padding(.top, 4)
        .background(Theme.bar.ignoresSafeArea(edges: .bottom))
        .overlay(alignment: .top) { Theme.barLine.frame(height: 1) }
    }
}

struct MiniPlayer: View {
    let player: Player
    let routes: PlayerRoutes?
    let go: (PlayerRoute) -> Void
    let open: () -> Void

    var body: some View {
        if let track = player.current {
            VStack(spacing: 0) {
                GeometryReader { geometry in
                    ZStack(alignment: .leading) {
                        Theme.line
                        Theme.accent.frame(width: geometry.size.width * player.progress)
                    }
                }
                .frame(height: 3)
                .accessibilityHidden(true)
                HStack(spacing: 12) {
                    Button(action: open) {
                        HStack(spacing: 12) {
                            Cover(path: track.artworkPath, size: 44)
                            VStack(alignment: .leading, spacing: 0) {
                                Text(track.title).lineLimit(1)
                                Text(player.subtitle)
                                    .font(Theme.small)
                                    .foregroundStyle(player.notice == nil ? Theme.soft : Theme.warning)
                                    .lineLimit(1)
                            }
                            Spacer(minLength: 0)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .contextMenu { GoToMenu(routes: routes, go: go) }
                    .accessibilityLabel("\(track.title), \(track.artist). Open now playing")
                    PlayButton(player: player, size: 44)
                    IconButton(icon: "next", label: "Next track", glyph: 20, action: player.next)
                        .disabled(!player.queue.hasNext)
                }
                .padding(EdgeInsets(top: 8, leading: 10, bottom: 8, trailing: 8))
            }
            .background(Theme.raised)
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .padding(.horizontal, 8)
            .padding(.bottom, 8)
        }
    }
}

struct GoToMenu: View {
    let routes: PlayerRoutes?
    let go: (PlayerRoute) -> Void

    var body: some View {
        if let routes {
            if routes.artists.count > 1 {
                Menu("Go to artist") {
                    ForEach(routes.artists, id: \.self) { route in
                        Button(route.title) { go(route) }
                    }
                }
            } else if let artist = routes.artists.first {
                Button("Go to artist") { go(artist) }
            }
            if let album = routes.album {
                Button("Go to album") { go(album) }
            }
        }
    }
}

struct PlayButton: View {
    let player: Player
    let size: CGFloat
    var light = false
    var glyph: CGFloat?

    var body: some View {
        Button(action: player.toggle) {
            ZStack {
                Circle().fill(light ? Theme.text : Theme.accent)
                if player.state == .loading || player.buffering {
                    ProgressView().tint(light ? Theme.stage : Theme.onAccent)
                } else {
                    Icon(player.state == .playing ? "pause" : "play", size: glyph ?? 18)
                        .foregroundStyle(light ? Theme.stage : Theme.onAccent)
                }
            }
            .frame(width: size, height: size)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(player.buffering ? "Buffering, pause" : player.state == .playing ? "Pause" : player.state == .loading ? "Loading" : "Play")
    }
}

extension Player {
    var progress: CGFloat {
        duration > 0 ? CGFloat(min(max(position / duration, 0), 1)) : 0
    }

    var searching: String {
        retrying.map { "Trying again (\($0.attempt) of \($0.of))…" } ?? "Finding a source…"
    }

    var subtitle: String {
        if retrying != nil { return searching }
        if buffering { return "Buffering…" }
        if let notice { return notice }
        guard let track = current else { return "" }
        if state == .loading { return searching }
        return sourceLabel.map { "\(track.artist) · from \($0)" } ?? track.artist
    }
}


struct Unreachable: View {
    @Environment(AppModel.self) private var model
    @State private var checking = false

    var body: some View {
        HStack(spacing: 10) {
            Circle().strokeBorder(Theme.warning, lineWidth: 1.5).frame(width: 10, height: 10)
            Text("Can't reach your server. Changes may not be saved yet.")
                .font(Theme.small)
                .foregroundStyle(Theme.text)
                .frame(maxWidth: .infinity, alignment: .leading)
            TextAction(title: checking ? "Checking…" : "Try again", inset: 8) {
                Task {
                    checking = true
                    await model.checkLock()
                    checking = false
                }
            }
            .disabled(checking)
        }
        .padding(.leading, 14)
        .padding(.trailing, 6)
        .background(RoundedRectangle(cornerRadius: 10).fill(Theme.field))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Theme.barLine))
        .padding(.horizontal, 8)
        .padding(.bottom, 8)
        .accessibilityElement(children: .contain)
        .task {
            while !Task.isCancelled, model.serverUnreachable {
                try? await Task.sleep(for: .seconds(10))
                await model.checkLock()
            }
        }
    }
}
