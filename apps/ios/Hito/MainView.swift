import HitoKit
import SwiftUI

enum Tab: CaseIterable, Hashable {
    case home, search, library, plugins

    var label: String {
        switch self {
        case .home: "Home"
        case .search: "Search"
        case .library: "Library"
        case .plugins: "Plugins"
        }
    }

    var icon: String {
        switch self {
        case .home: "home"
        case .search: "search"
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

    var body: some View {
        ZStack {
            Theme.ground.ignoresSafeArea()
            switch tab {
            case .home:
                NavigationStack {
                    HomeView(user: user, openSearch: { tab = .search })
                        .browsing()
                }
            case .search:
                NavigationStack { SearchView().browsing() }
            case .library:
                NavigationStack { LibraryView().browsing() }
            case .plugins:
                NavigationStack { PluginsView().browsing() }
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            VStack(spacing: 0) {
                if let player = model.player, player.current != nil {
                    MiniPlayer(player: player) { showingNowPlaying = true }
                }
                TabBar(tab: $tab)
            }
        }
        .foregroundStyle(Theme.text)
        .fullScreenCover(isPresented: $showingNowPlaying) {
            if let player = model.player {
                NowPlayingView(player: player, openQueue: { showingQueue = true })
                    .sheet(isPresented: $showingQueue) { QueueSheet(player: player) }
            }
        }
    }
}

extension View {
    func browsing() -> some View {
        navigationDestination(for: Album.self) { AlbumView(album: $0).pushed() }
            .navigationDestination(for: TrackList.self) { TrackListView(list: $0).pushed() }
            .navigationDestination(for: Playlist.self) { PlaylistView(playlist: $0).pushed() }
            .navigationDestination(for: PluginConnection.self) { PluginDetailView(connection: $0).pushed(back: "Plugins") }
            .navigationDestination(for: SettingsPage.self) { page in
                switch page {
                case .root: SettingsView().pushed()
                case .profile: ProfileView().pushed(page.title)
                case .privacy: PrivacyView().pushed(page.title)
                case .playback: PlaybackSettingsView().pushed(page.title)
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

    func pushed(_ title: String? = nil, back: String? = nil) -> some View {
        modifier(Pushed(title: title, back: back))
    }
}

struct Pushed: ViewModifier {
    @Environment(\.dismiss) private var dismiss
    let title: String?
    let back: String?

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
                .background(Theme.ground)
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
            }
        }
        .padding(.horizontal, 4)
        .padding(.top, 4)
        .background(Theme.bar.ignoresSafeArea(edges: .bottom))
        .overlay(alignment: .top) { Theme.barLine.frame(height: 1) }
    }
}

struct MiniPlayer: View {
    let player: Player
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

struct PlayButton: View {
    let player: Player
    let size: CGFloat
    var light = false
    var glyph: CGFloat?

    var body: some View {
        Button(action: player.toggle) {
            ZStack {
                Circle().fill(light ? Theme.text : Theme.accent)
                if player.state == .loading {
                    ProgressView().tint(light ? Theme.stage : Theme.onAccent)
                } else {
                    Icon(player.state == .playing ? "pause" : "play", size: glyph ?? 18)
                        .foregroundStyle(light ? Theme.stage : Theme.onAccent)
                }
            }
            .frame(width: size, height: size)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(player.state == .playing ? "Pause" : player.state == .loading ? "Loading" : "Play")
    }
}

extension Player {
    var progress: CGFloat {
        duration > 0 ? CGFloat(min(max(position / duration, 0), 1)) : 0
    }

    var subtitle: String {
        if let notice { return notice }
        guard let track = current else { return "" }
        if state == .loading { return "Finding a source…" }
        return sourceLabel.map { "\(track.artist) · from \($0)" } ?? track.artist
    }
}

