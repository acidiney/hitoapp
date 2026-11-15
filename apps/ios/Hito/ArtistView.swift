import HitoKit
import SwiftUI

struct ArtistView: View {
    @Environment(AppModel.self) private var model
    let artist: Artist
    @State private var detail: ArtistPage.Detail?
    @State private var problem: String?

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 22) {
                header
                if let detail {
                    if !detail.tracks.isEmpty {
                        section("Tracks", detail.tracks, from: detail.name)
                        if detail.tracksTotal > detail.tracks.count {
                            NavigationLink(value: TrackList.artist(artist)) {
                                Text("All \(detail.tracksTotal) tracks").font(Theme.smallStrong).foregroundStyle(Theme.focus).frame(minHeight: 44)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    if !detail.albums.isEmpty {
                        Text("Albums in your library").font(Theme.section).accessibilityAddTraits(.isHeader)
                        LazyVGrid(columns: [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)], spacing: 16) {
                            ForEach(detail.albums) { album in
                                NavigationLink(value: album) { AlbumCard(album: album) }.buttonStyle(.plain)
                            }
                        }
                    }
                    if !detail.appearances.isEmpty {
                        section("Appears on", detail.appearances, from: "\(detail.name) appearances", note: "Recordings that credit this artist as a guest")
                    }
                } else if let problem {
                    Text(problem).font(Theme.small).foregroundStyle(Theme.warning)
                } else {
                    ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 80)
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
        .task { await load() }
        .refreshable { await load() }
    }

    private var header: some View {
        HStack(alignment: .center, spacing: 16) {
            ZStack {
                Theme.tile
                Icon("person", size: 40).foregroundStyle(Theme.coverMark)
            }
            .frame(width: 104, height: 104)
            .clipShape(Circle())
            VStack(alignment: .leading, spacing: 4) {
                Text("Artist").font(Theme.label).foregroundStyle(Theme.quiet)
                Text(detail?.name ?? artist.name).font(Theme.title).lineLimit(3).accessibilityAddTraits(.isHeader)
                Text(detail?.stats ?? ArtistPage.counts(tracks: artist.trackCount, albums: artist.albumCount, featured: 0))
                    .font(Theme.small).foregroundStyle(Theme.soft)
                if let tracks = detail?.tracks, !tracks.isEmpty {
                    HStack(spacing: 8) {
                        PlayAllButton(label: "Play") { model.player?.play(tracks, at: 0, from: artist.name) }
                        Button {
                            model.player?.play(tracks.shuffled(), at: 0, from: artist.name)
                        } label: {
                            Icon("shuffle", size: 20).foregroundStyle(Theme.text).frame(width: 48, height: 48).overlay(Circle().strokeBorder(Theme.line))
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Shuffle \(artist.name)")
                    }
                    .padding(.top, 6)
                }
            }
            Spacer(minLength: 0)
        }
    }

    private func section(_ title: String, _ tracks: [Track], from context: String, note: String? = nil) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(Theme.section).accessibilityAddTraits(.isHeader)
            if let note { Text(note).font(Theme.small).foregroundStyle(Theme.quiet) }
            ForEach(Array(tracks.enumerated()), id: \.element.id) { index, track in
                HStack(spacing: 0) {
                    Button {
                        model.player?.play(tracks, at: index, from: context)
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
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            detail = try await ArtistPage(api: api).load(artist.id)
            problem = nil
        } catch {
            problem = "This artist couldn't load. Pull to try again."
        }
    }
}
