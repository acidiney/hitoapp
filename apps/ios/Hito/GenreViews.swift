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
                                Text(genre.count).font(Theme.caption).monospacedDigit()
                            }
                            .foregroundStyle(Theme.text)
                            .padding(14)
                            .frame(maxWidth: .infinity, minHeight: 104, alignment: .leading)
                            .background(RoundedRectangle(cornerRadius: 10).fill(Self.tones[index % Self.tones.count]))
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("\(genre.title), \(genre.count)")
                        .collectionActions(genre.title, preview: ItemPreview(path: nil, title: genre.title, detail: genre.count)) {
                            try await Genres(api: $0).detail(genre.key).tracks
                        }
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
                HStack(alignment: .center, spacing: 14) {
                    GenreCover(albums: detail?.albums ?? [], size: 96)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Genre").font(Theme.label).foregroundStyle(Theme.soft)
                        Text(genre.title).font(Theme.display).tracking(Theme.titleTracking).lineLimit(3).accessibilityAddTraits(.isHeader)
                        Text("\((detail?.genre ?? genre).count) in your library").font(Theme.small).foregroundStyle(Theme.soft)
                    }
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
                                        .albumActions(album)
                                }
                            }
                        }
                        .scrollIndicators(.hidden)
                        .padding(.trailing, -16)
                    }
                    Text("Tracks").font(Theme.section).padding(.top, 8).accessibilityAddTraits(.isHeader)
                    if detail.genre.trackCount > tracks.count {
                        Text("Showing \(tracks.count) of \(detail.genre.shortCount) tracks.").font(Theme.small).foregroundStyle(Theme.soft)
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
                            .trackActions(track)
                        }
                    }
                    .padding(.trailing, -8)
                    if paged == nil ? detail.mayHaveMore : cursor != nil {
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
        guard let api = model.api, let detail else { return }
        loadingMore = true
        moreFailed = false
        defer { loadingMore = false }
        do {
            let page = try await Genres(api: api).tracks(genre.key, after: paged == nil ? nil : cursor, past: paged == nil ? shown(detail).count : 0)
            let known = Set((paged ?? []).map(\.id))
            paged = (paged ?? []) + page.items.filter { !known.contains($0.id) }
            cursor = page.next
        } catch is CancellationError {
        } catch HitoAPI.Problem.refused(422) {
            paged = nil
            cursor = nil
            moreFailed = true
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
            paged = nil
            cursor = nil
            moreFailed = false
        } catch is CancellationError {
        } catch {
            failed = true
            model.handle(error)
        }
    }
}

struct GenreCover: View {
    let albums: [Album]
    let size: CGFloat

    var body: some View {
        if albums.count >= 4 {
            Grid(horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    tile(albums[0])
                    tile(albums[1])
                }
                GridRow {
                    tile(albums[2])
                    tile(albums[3])
                }
            }
            .frame(width: size, height: size)
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .accessibilityHidden(true)
        } else if let first = albums.first {
            Cover(path: first.artworkPath, size: size, radius: 10)
        } else {
            Icon("note", size: 40)
                .foregroundStyle(Theme.coverMark)
                .frame(width: size, height: size)
                .background(RoundedRectangle(cornerRadius: 10).fill(Theme.tile))
                .accessibilityHidden(true)
        }
    }

    private func tile(_ album: Album) -> some View {
        Cover(path: album.artworkPath, size: size / 2, radius: 0)
    }
}
