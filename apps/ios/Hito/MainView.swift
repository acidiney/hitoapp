import HitoKit
import SwiftUI

enum Tab: CaseIterable, Hashable {
    case home, search, library

    var label: String {
        switch self {
        case .home: "Home"
        case .search: "Search"
        case .library: "Library"
        }
    }

    var icon: String {
        switch self {
        case .home: "home"
        case .search: "search"
        case .library: "library"
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
    @State private var showingAccount = false

    var body: some View {
        ZStack {
            Theme.ground.ignoresSafeArea()
            switch tab {
            case .home:
                NavigationStack {
                    HomeView(user: user, openSearch: { tab = .search }, openAccount: { showingAccount = true })
                        .browsing()
                }
            case .search:
                NavigationStack { SearchView().browsing() }
            case .library:
                NavigationStack { LibraryView().browsing() }
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
        .sheet(isPresented: $showingAccount) {
            AccountSheet(address: address, user: user)
        }
    }
}

extension View {
    func browsing() -> some View {
        navigationDestination(for: Album.self) { AlbumView(album: $0).pushed() }
            .navigationDestination(for: TrackList.self) { TrackListView(list: $0).pushed() }
            .navigationDestination(for: Playlist.self) { PlaylistView(playlist: $0).pushed() }
            .page()
    }

    func page() -> some View {
        toolbar(.hidden, for: .navigationBar)
            .background(Theme.ground.ignoresSafeArea())
            .foregroundStyle(Theme.text)
    }

    func pushed() -> some View {
        modifier(Pushed())
    }
}

struct Pushed: ViewModifier {
    @Environment(\.dismiss) private var dismiss

    func body(content: Content) -> some View {
        content
            .safeAreaInset(edge: .top, spacing: 0) {
                HStack {
                    IconButton(icon: "chevron-left", label: "Back", glyph: 22) { dismiss() }
                    Spacer()
                }
                .padding(.horizontal, 6)
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

struct AccountSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let address: ServerAddress
    let user: User
    @State private var confirmingChange = false

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            AccountBadge(name: user.displayName ?? user.username, admin: user.admin, detail: user.username)
            VStack(alignment: .leading, spacing: 4) {
                Text("This app uses \(address.origin.absoluteString)").foregroundStyle(Theme.soft)
                TextAction(title: "Change…") { confirmingChange = true }
            }
            TextAction(title: "Sign out") {
                dismiss()
                model.signOut()
            }
            Spacer()
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.field.ignoresSafeArea())
        .foregroundStyle(Theme.text)
        .presentationDetents([.medium])
        .presentationDragIndicator(.visible)
        .confirmationDialog("Use a different place?", isPresented: $confirmingChange, titleVisibility: .visible) {
            Button("Change") {
                dismiss()
                model.changeServer()
            }
        } message: {
            Text("The music stops and the first screen opens. You stay signed in to \(address.host).")
        }
    }
}
