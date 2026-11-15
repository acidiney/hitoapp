import HitoKit
import SwiftUI

struct CatalogRelease: Hashable {
    let group: ArtistGroup
    let release: ReleaseGroup
}

struct CatalogAlbumView: View {
    @Environment(AppModel.self) private var model
    let route: CatalogRelease
    @State private var options: [EditionOption]?
    @State private var chosen: EditionOption?
    @State private var edition: EditionRead?
    @State private var problem: String?
    @State private var adding: Set<String> = []
    @State private var notes: [String: String] = [:]

    private var anchor: CatalogAnchor {
        CatalogAnchor(recordingId: route.group.recordingId, associationId: route.group.associationId)
    }

    private var complete: CompleteAlbum? {
        if case let .available(album) = edition { album } else { nil }
    }

    private var playable: [Track] { complete?.tracks.compactMap(\.owned) ?? [] }

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 16) {
                header
                if let problem {
                    Text(problem).font(Theme.small).foregroundStyle(Theme.warning)
                    TextAction(title: "Try again") { Task { await load() } }
                } else if case let .unavailable(reason) = edition {
                    Text(reason).font(Theme.small).foregroundStyle(Theme.quiet)
                } else if let complete {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(complete.tracks) { track in
                            if let owned = track.owned {
                                ownedRow(owned)
                            } else {
                                missingRow(track, in: complete)
                            }
                        }
                    }
                } else {
                    HStack(spacing: 8) {
                        ProgressView().tint(Theme.soft).controlSize(.small)
                        Text("Loading album…").font(Theme.small).foregroundStyle(Theme.quiet)
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
        .task { if options == nil { await load() } }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(kind).font(Theme.label).foregroundStyle(Theme.quiet)
                Text(route.release.title).font(Theme.title).tracking(Theme.titleTracking).lineLimit(3).accessibilityAddTraits(.isHeader)
                Text([route.group.name, route.release.year, complete?.summary].compactMap { $0 }.joined(separator: " · "))
                    .font(Theme.small).foregroundStyle(Theme.soft)
            }
            HStack(spacing: 12) {
                if !playable.isEmpty {
                    PlayAllButton(label: "Play") { model.player?.play(playable, at: 0, from: route.release.title) }
                }
                if let options, options.count > 1 {
                    editionMenu(options)
                }
            }
        }
        .padding(.top, 8)
    }

    private var kind: String {
        switch route.release.kind {
        case .album: "Album"
        case .ep: "EP"
        case .single: "Single"
        case nil: "Release"
        }
    }

    private func editionMenu(_ options: [EditionOption]) -> some View {
        Menu {
            ForEach(options) { option in
                Button { Task { await read(option) } } label: {
                    if option.id == chosen?.id {
                        Label(option.label, systemImage: "checkmark")
                    } else {
                        Text(option.label)
                    }
                }
            }
        } label: {
            HStack(spacing: 8) {
                Text("Edition").foregroundStyle(Theme.quiet)
                Text(chosen?.name ?? "Choose").font(Theme.bodyMedium).lineLimit(1)
                Icon("chevron-down", size: 14)
            }
            .font(Theme.small)
            .foregroundStyle(Theme.text)
            .padding(.horizontal, 16)
            .frame(minHeight: 44)
            .overlay(Capsule().strokeBorder(Theme.line))
            .contentShape(Capsule())
        }
        .accessibilityLabel("Edition, \(chosen?.name ?? "not chosen")")
    }

    private func ownedRow(_ track: Track) -> some View {
        HStack(spacing: 4) {
            Button {
                let list = playable
                model.player?.play(list, at: list.firstIndex { $0.id == track.id } ?? 0, from: route.release.title)
            } label: {
                AlbumTrackRow(track: track, isCurrent: model.player?.current?.id == track.id)
            }
            .buttonStyle(.plain)
            Icon("check", size: 16)
                .foregroundStyle(Theme.focus)
                .frame(width: 44, height: 44)
                .accessibilityLabel("In your library")
            Menu { TrackMenu(track: track) } label: {
                Icon("more").foregroundStyle(Theme.quiet).frame(width: 44, height: 44).contentShape(Circle())
            }
            .accessibilityLabel("More actions for \(track.title)")
        }
    }

    private func missingRow(_ track: EditionTrack, in complete: CompleteAlbum) -> some View {
        let finding = adding.contains(track.id)
        let canFind = complete.addable.contains { $0.id == track.id } && notes[track.id] == nil
        return HStack(spacing: 4) {
            VStack(alignment: .leading, spacing: 0) {
                Text(track.title).foregroundStyle(Theme.soft).lineLimit(1)
                HStack(spacing: 6) {
                    Circle()
                        .strokeBorder(finding ? Theme.quiet : Theme.missingRing, style: StrokeStyle(lineWidth: 1.5, dash: finding ? [2, 2] : []))
                        .frame(width: 9, height: 9)
                    Text(finding ? "Finding audio…" : notes[track.id] ?? track.note ?? "")
                        .foregroundStyle(finding ? Theme.quiet : Theme.missing)
                        .lineLimit(1)
                }
                .font(Theme.small)
            }
            .frame(maxWidth: .infinity, minHeight: 60, alignment: .leading)
            if finding {
                ProgressView().tint(Theme.soft).frame(width: 44, height: 44)
            } else if canFind {
                Button { Task { await find(track, in: complete) } } label: {
                    Icon("plus", size: 16).foregroundStyle(Theme.soft).frame(width: 44, height: 44).contentShape(Circle())
                }
                .buttonStyle(.plain)
                .disabled(!adding.isEmpty)
                .accessibilityLabel("Add \(track.title) to your library")
            }
        }
        .accessibilityElement(children: .combine)
    }

    private func load() async {
        guard let api = model.api else { return }
        problem = nil
        do {
            let found = try await AlbumEditions(api: api).editions(of: route.release.id, anchor: anchor)
            options = found
            if let first = found.first {
                await read(first)
            } else {
                edition = .unavailable("No editions found for this release.")
            }
        } catch let refused as EditionOption.Unavailable {
            options = []
            edition = .unavailable(refused.reason)
        } catch is CancellationError {
        } catch {
            problem = "This album couldn't load."
            model.handle(error)
        }
    }

    private func read(_ option: EditionOption) async {
        guard let api = model.api else { return }
        chosen = option
        edition = nil
        notes = [:]
        do {
            edition = try await AlbumEditions(api: api).catalog(option, anchor: anchor)
        } catch is CancellationError {
        } catch {
            problem = "This edition couldn't load."
            model.handle(error)
        }
    }

    private func find(_ track: EditionTrack, in complete: CompleteAlbum) async {
        guard let api = model.api else { return }
        adding.insert(track.id)
        defer { adding.remove(track.id) }
        do {
            switch try await AlbumEditions(api: api).find(track, of: complete, anchor: anchor) {
            case let .saved(recording):
                if case let .available(current) = edition { edition = .available(current.replacing(track.id, with: recording)) }
            case let .missing(reason):
                notes[track.id] = reason
            }
        } catch is CancellationError {
        } catch {
            notes[track.id] = PluginTrouble.message(error, otherwise: "Couldn't find audio")
            model.handle(error)
        }
    }
}
