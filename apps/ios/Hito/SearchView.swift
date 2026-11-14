import HitoKit
import SwiftUI

struct SearchView: View {
    @Environment(AppModel.self) private var model
    @State private var text = ""
    @State private var tracks = Feed<Track>()
    @State private var artist: Artist?
    @FocusState private var focused: Bool

    var query: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                Icon("search").foregroundStyle(Theme.quiet)
                TextField("", text: $text, prompt: Text("Search your library").foregroundStyle(Theme.quiet))
                    .accessibilityLabel("Search")
                    .focused($focused)
                    .submitLabel(.search)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                if !text.isEmpty {
                    IconButton(icon: "close", label: "Clear search", color: Theme.quiet) { text = "" }
                }
            }
            .padding(.leading, 16)
            .padding(.trailing, 4)
            .frame(minHeight: 48)
            .background(Capsule().fill(Theme.field))
            .overlay(Capsule().strokeBorder(focused ? Theme.accent : Theme.line, lineWidth: focused ? 2 : 1))
            .padding(.horizontal, 16)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    if query.isEmpty {
                        Text("Find tracks and artists in your library.")
                            .font(Theme.small)
                            .foregroundStyle(Theme.soft)
                    } else if tracks.isEmpty && artist == nil {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Nothing in your library matches “\(query)”").font(Theme.bodyStrong)
                            Text("Check the spelling, or try fewer words.").font(Theme.small).foregroundStyle(Theme.soft)
                        }
                    } else {
                        if let artist { TopResult(artist: artist) }
                        if !tracks.items.isEmpty {
                            Text("Tracks").font(Theme.heading).padding(.top, 4).accessibilityAddTraits(.isHeader)
                        }
                        VStack(spacing: 0) {
                            ForEach(Array(tracks.items.enumerated()), id: \.element.id) { index, track in
                                HStack(spacing: 0) {
                                    Button {
                                        model.player?.play(tracks.items, at: index, from: "Search")
                                    } label: {
                                        SearchRow(track: track, isCurrent: model.player?.current?.id == track.id)
                                    }
                                    .buttonStyle(.plain)
                                    .disabled(!track.isPlayable)
                                    Menu { TrackMenu(track: track) } label: {
                                        Icon("more-vertical").foregroundStyle(Theme.quiet).frame(width: 44, height: 44).contentShape(Circle())
                                    }
                                    .accessibilityLabel("More actions for \(track.title)")
                                }
                            }
                            FeedFooter(feed: tracks)
                        }
                        .padding(.trailing, -8)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 16)
            }
            .scrollIndicators(.hidden)
        }
        .task(id: query) {
            guard !query.isEmpty, let api = model.api else {
                artist = nil
                return
            }
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            let library = Library(api: api)
            let search = query
            tracks.reset { try await library.tracks(offset: $0, search: search) }
            async let found = try? library.artists(offset: 0, search: search).items.first
            await tracks.more(model)
            let match = await found
            guard !Task.isCancelled else { return }
            artist = match
        }
        .onAppear { focused = true }
    }
}

struct TopResult: View {
    @Environment(AppModel.self) private var model
    let artist: Artist

    var body: some View {
        HStack(spacing: 14) {
            ZStack {
                Theme.tile
                Icon("person", size: 28).foregroundStyle(Theme.coverMark)
            }
            .frame(width: 64, height: 64)
            .clipShape(Circle())
            .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 0) {
                Text("Top result").font(Theme.label).foregroundStyle(Theme.focus)
                Text(artist.name).font(Theme.result).lineLimit(1)
                Text("Artist · \(artist.trackCount) \(artist.trackCount == 1 ? "track" : "tracks") in your library")
                    .font(Theme.small)
                    .foregroundStyle(Theme.quiet)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
            Button(action: play) {
                Icon("play", size: 16)
                    .foregroundStyle(Theme.onAccent)
                    .frame(width: 44, height: 44)
                    .background(Circle().fill(Theme.accent))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Play \(artist.name)")
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
    }

    private func play() {
        guard let api = model.api else { return }
        let id = artist.id
        let name = artist.name
        Task {
            do {
                let page = try await Library(api: api).tracks(offset: 0, artist: id)
                model.player?.play(page.items, at: page.items.firstIndex { $0.isPlayable } ?? 0, from: name)
            } catch {
                model.handle(error)
            }
        }
    }
}
