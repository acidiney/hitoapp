import HitoKit
import SwiftUI

struct GenreTiles: View {
    let genres: [Genre]
    @State private var all = false

    static let preview = 12
    static let tones: [Color] = [
        Color(red: 0x7a / 255, green: 0x3b / 255, blue: 0x1d / 255),
        Color(red: 0x1f / 255, green: 0x4a / 255, blue: 0x43 / 255),
        Color(red: 0x2b / 255, green: 0x2f / 255, blue: 0x6b / 255),
        Color(red: 0x5a / 255, green: 0x2a / 255, blue: 0x4f / 255),
        Color(red: 0x4a / 255, green: 0x4a / 255, blue: 0x1f / 255),
        Color(red: 0x25 / 255, green: 0x40 / 255, blue: 0x5f / 255),
    ]

    var body: some View {
        if !genres.isEmpty {
            let shown = all ? genres : Array(genres.prefix(Self.preview))
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text("Genres you own").font(Theme.section).accessibilityAddTraits(.isHeader)
                    Spacer(minLength: 8)
                    if genres.count > Self.preview {
                        TextAction(title: all ? "Show fewer" : "Show all \(genres.count)") { all.toggle() }
                    }
                }
                LazyVGrid(columns: [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)], spacing: 12) {
                    ForEach(Array(shown.enumerated()), id: \.element.id) { index, genre in
                        NavigationLink(value: genre) {
                            VStack(alignment: .leading, spacing: 2) {
                                Spacer(minLength: 0)
                                Text(genre.title).font(Theme.genreName).lineLimit(2).multilineTextAlignment(.leading)
                                Text(genre.count).font(Theme.small).foregroundStyle(Theme.text.opacity(0.8))
                            }
                            .foregroundStyle(Theme.text)
                            .padding(14)
                            .frame(maxWidth: .infinity, minHeight: 104, alignment: .leading)
                            .background(RoundedRectangle(cornerRadius: 10).fill(Self.tones[index % Self.tones.count]))
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("\(genre.title), \(genre.count)")
                    }
                }
            }
        }
    }
}

struct GenreView: View {
    @Environment(AppModel.self) private var model
    let genre: Genre
    @State private var detail: GenreDetail?
    @State private var failed = false
    @State private var paged: [Track]?
    @State private var cursor: String?
    @State private var loadingMore = false
    @State private var moreFailed = false

    private func shown(_ detail: GenreDetail) -> [Track] { paged ?? detail.tracks }

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 14) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Genre").font(Theme.label).foregroundStyle(Theme.soft)
                    Text(genre.title).font(Theme.display).tracking(Theme.titleTracking).lineLimit(3).accessibilityAddTraits(.isHeader)
                    Text("\(genre.count) in your library").font(Theme.small).foregroundStyle(Theme.soft)
                }
                if failed {
                    HStack(spacing: 8) {
                        Text("This genre didn't load.").foregroundStyle(Theme.soft)
                        TextAction(title: "Try again") { Task { await load() } }
                    }
                    .font(Theme.small)
                } else if let detail {
                    let tracks = shown(detail)
                    if !tracks.isEmpty {
                        HStack(spacing: 8) {
                            PlayAllButton(label: "Play") { model.player?.play(tracks, at: 0, from: genre.title) }
                            OutlineButton(title: "Shuffle") {
                                let mixed = tracks.shuffled()
                                model.player?.play(mixed, at: 0, from: genre.title)
                            }
                        }
                    }
                    if !detail.albums.isEmpty {
                        Text("Albums").font(Theme.section).padding(.top, 8).accessibilityAddTraits(.isHeader)
                        ScrollView(.horizontal) {
                            LazyHStack(alignment: .top, spacing: 12) {
                                ForEach(detail.albums) { album in
                                    NavigationLink(value: album) { AlbumCard(album: album, width: 136) }
                                        .buttonStyle(.plain)
                                }
                            }
                        }
                        .scrollIndicators(.hidden)
                        .padding(.trailing, -16)
                    }
                    Text("Tracks").font(Theme.section).padding(.top, 8).accessibilityAddTraits(.isHeader)
                    if genre.trackCount > tracks.count {
                        Text("Showing \(tracks.count) of \(genre.shortCount) tracks.").font(Theme.small).foregroundStyle(Theme.soft)
                    }
                    VStack(spacing: 0) {
                        ForEach(Array(tracks.enumerated()), id: \.element.id) { index, track in
                            HStack(spacing: 0) {
                                Button {
                                    model.player?.play(tracks, at: index, from: genre.title)
                                } label: {
                                    LibraryRow(track: track, isCurrent: model.player?.current?.id == track.id)
                                }
                                .buttonStyle(.plain)
                                Menu { TrackMenu(track: track) } label: {
                                    Icon("more-vertical").foregroundStyle(Theme.quiet).frame(width: 44, height: 44).contentShape(Circle())
                                }
                                .accessibilityLabel("More actions for \(track.title)")
                            }
                        }
                    }
                    .padding(.trailing, -8)
                    if genre.trackCount > tracks.count && (paged == nil || cursor != nil) {
                        if moreFailed {
                            Text("More tracks didn't load.").font(Theme.small).foregroundStyle(Theme.soft)
                        }
                        TextAction(title: loadingMore ? "Loading…" : moreFailed ? "Try again" : "Load more tracks") { Task { await loadMore() } }
                            .disabled(loadingMore)
                    }
                } else {
                    ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 16)
        }
        .scrollIndicators(.hidden)
        .refreshable { await load() }
        .task { if detail == nil { await load() } }
    }

    private func loadMore() async {
        guard let api = model.api else { return }
        loadingMore = true
        moreFailed = false
        defer { loadingMore = false }
        do {
            let page = try await Genres(api: api).tracks(genre.key, after: paged == nil ? nil : cursor)
            let known = Set((paged ?? []).map(\.id))
            paged = (paged ?? []) + page.items.filter { !known.contains($0.id) }
            cursor = page.next
        } catch is CancellationError {
        } catch {
            moreFailed = true
            model.handle(error)
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        failed = false
        do {
            detail = try await Genres(api: api).detail(genre.key)
        } catch is CancellationError {
        } catch {
            failed = true
            model.handle(error)
        }
    }
}
