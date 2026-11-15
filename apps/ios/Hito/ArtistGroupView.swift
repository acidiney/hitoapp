import HitoKit
import SwiftUI

struct ArtistGroupView: View {
    @Environment(AppModel.self) private var model
    let group: ArtistGroup
    @State private var detail: ArtistGroupDetail?
    @State private var problem: String?
    @State private var releases: [ReleaseGroup] = []
    @State private var owned: [String: String] = [:]
    @State private var continuation: String?
    @State private var discographyNote: String?
    @State private var loadingReleases = false

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 22) {
                header
                if let detail {
                    if !detail.tracks.isEmpty {
                        tracks(detail)
                    }
                    if !detail.albums.isEmpty {
                        Text("Albums in your library").font(Theme.section).accessibilityAddTraits(.isHeader)
                        LazyVGrid(columns: [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)], spacing: 16) {
                            ForEach(detail.albums) { album in
                                NavigationLink(value: album) { AlbumCard(album: album) }.buttonStyle(.plain)
                            }
                        }
                    }
                    discography(detail)
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
        .task { if detail == nil { await load() } }
        .refreshable { await load() }
    }

    private var header: some View {
        HStack(alignment: .center, spacing: 16) {
            Cover(path: group.artworkPath, size: 104, radius: 52)
            VStack(alignment: .leading, spacing: 4) {
                Text("Artist").font(Theme.label).foregroundStyle(Theme.quiet)
                Text(group.name).font(Theme.title).tracking(Theme.titleTracking).lineLimit(3).accessibilityAddTraits(.isHeader)
                Text(detail?.stats ?? ArtistPage.counts(tracks: group.recordingCount, albums: group.releaseCount, featured: 0))
                    .font(Theme.small).foregroundStyle(Theme.soft)
                if let tracks = detail?.tracks, !tracks.isEmpty {
                    HStack(spacing: 8) {
                        PlayAllButton(label: "Play") { model.player?.play(tracks, at: 0, from: group.name) }
                        Button {
                            model.player?.play(tracks.shuffled(), at: 0, from: group.name)
                        } label: {
                            Icon("shuffle", size: 20).foregroundStyle(Theme.text).frame(width: 48, height: 48).overlay(Circle().strokeBorder(Theme.line))
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Shuffle \(group.name)")
                    }
                    .padding(.top, 6)
                }
            }
            Spacer(minLength: 0)
        }
    }

    private func tracks(_ detail: ArtistGroupDetail) -> some View {
        let shown = Array(detail.tracks.prefix(5))
        return VStack(alignment: .leading, spacing: 4) {
            Text("Tracks").font(Theme.section).accessibilityAddTraits(.isHeader)
            ForEach(Array(shown.enumerated()), id: \.element.id) { index, track in
                HStack(spacing: 0) {
                    Button {
                        model.player?.play(detail.tracks, at: index, from: group.name)
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

    @ViewBuilder
    private func discography(_ detail: ArtistGroupDetail) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Discography").font(Theme.section).accessibilityAddTraits(.isHeader)
            if let discographyNote {
                Text(discographyNote).font(Theme.small).foregroundStyle(Theme.quiet)
            } else if releases.isEmpty && loadingReleases {
                HStack(spacing: 8) {
                    ProgressView().tint(Theme.soft).controlSize(.small)
                    Text("Loading discography…").font(Theme.small).foregroundStyle(Theme.quiet)
                }
            } else if releases.isEmpty {
                Text("No releases listed for this artist. Your own albums are still listed.").font(Theme.small).foregroundStyle(Theme.quiet)
            }
            ForEach(ReleaseGroup.sections(releases), id: \.title) { section in
                VStack(alignment: .leading, spacing: 0) {
                    Text(section.title).font(Theme.smallStrong).foregroundStyle(Theme.soft).padding(.bottom, 4)
                    ForEach(section.items) { release in
                        row(release, in: detail)
                    }
                }
            }
            if continuation != nil {
                Button {
                    Task { await more() }
                } label: {
                    Text(loadingReleases ? "Loading more releases…" : "Load more releases")
                        .font(Theme.smallStrong).foregroundStyle(Theme.focus).frame(minHeight: 44)
                }
                .buttonStyle(.plain)
                .disabled(loadingReleases)
            }
        }
    }

    @ViewBuilder
    private func row(_ release: ReleaseGroup, in detail: ArtistGroupDetail) -> some View {
        let label = HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(release.title).font(Theme.body).foregroundStyle(Theme.text).lineLimit(2)
                if let year = release.year {
                    Text(year).font(Theme.small).foregroundStyle(Theme.quiet)
                }
            }
            Spacer(minLength: 8)
            if owned[release.id] != nil {
                Text("In your library").font(Theme.small).foregroundStyle(Theme.focus)
            }
        }
        .frame(minHeight: 52)
        .contentShape(Rectangle())
        if let releaseId = owned[release.id] {
            NavigationLink(value: libraryAlbum(releaseId, release, in: detail)) { label }
                .buttonStyle(.plain)
                .accessibilityHint("Opens the album in your library")
        } else {
            NavigationLink(value: CatalogRelease(group: group, release: release)) { label }
                .buttonStyle(.plain)
        }
    }

    private func libraryAlbum(_ id: String, _ release: ReleaseGroup, in detail: ArtistGroupDetail) -> Album {
        detail.albums.first { $0.id == id }
            ?? Album(id: id, title: release.title, artist: group.name, trackCount: 0, year: release.year.flatMap(Int.init), durationMs: nil)
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            detail = try await ArtistGroups(api: api).detail(group.id)
            problem = nil
        } catch {
            problem = "This artist couldn't load. Pull to try again."
            return
        }
        releases = []
        owned = [:]
        continuation = nil
        discographyNote = nil
        await read(from: nil)
    }

    private func more() async {
        await read(from: continuation)
    }

    private func read(from cursor: String?) async {
        guard let api = model.api else { return }
        let groups = ArtistGroups(api: api)
        loadingReleases = true
        defer { loadingReleases = false }
        do {
            switch try await groups.discography(group, continuation: cursor) {
            case let .available(items, next):
                releases += items
                continuation = next
                if let found = try? await groups.library(group, releaseGroups: items.map(\.id)) {
                    owned.merge(found) { first, _ in first }
                }
            case let .unavailable(reason):
                discographyNote = reason
                continuation = nil
            }
        } catch {
            discographyNote = "Couldn't load the discography. Your own albums are still listed."
        }
    }
}
