import HitoKit
import SwiftUI

struct AudiobookRoute: Hashable {
    let playlist: Playlist
}

struct AudiobooksSection: View {
    @Environment(AppModel.self) private var model
    @State private var books: [Playlist]?
    @State private var next: String?
    @State private var playlists: [Playlist] = []
    @State private var adding: String?
    @State private var problem: String?

    var candidates: [Playlist] {
        let taken = Set((books ?? []).map(\.id))
        return playlists.filter { $0.kind == .local && !taken.contains($0.id) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 10) {
                Text("Add an audiobook").font(Theme.section).accessibilityAddTraits(.isHeader)
                Text("Group an editable playlist into ordered chapters and it will appear here.").font(Theme.small).foregroundStyle(Theme.soft)
                Menu {
                    ForEach(candidates) { playlist in
                        Button(playlist.name) { Task { await add(playlist) } }
                    }
                } label: {
                    Text(adding != nil ? "Adding audiobook…" : "Add audiobook from a playlist")
                        .font(Theme.bodyStrong)
                        .foregroundStyle(Theme.onAccent)
                        .padding(.horizontal, 18)
                        .frame(minHeight: 44)
                        .background(Capsule().fill(Theme.accent))
                }
                .disabled(adding != nil || candidates.isEmpty)
                .opacity(candidates.isEmpty ? 0.5 : 1)
                if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.warning) }
            }
            .padding(14)
            .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
            if let books {
                if books.isEmpty {
                    Text("No audiobooks yet").font(Theme.bodyStrong)
                } else {
                    Text("Audiobooks").font(Theme.section).accessibilityAddTraits(.isHeader)
                    ForEach(books) { book in
                        NavigationLink(value: AudiobookRoute(playlist: book)) {
                            HStack(spacing: 12) {
                                Cover(path: book.artworkPath, size: 44, mark: "book")
                                Text(book.name).lineLimit(2)
                                Spacer(minLength: 8)
                                Icon("chevron-right", size: 16).foregroundStyle(Theme.quiet)
                            }
                            .frame(minHeight: 60)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                    if next != nil {
                        TextAction(title: "Show more audiobooks") { Task { await loadMore() } }
                    }
                }
            } else {
                Text("Loading audiobooks…").font(Theme.small).foregroundStyle(Theme.soft)
            }
        }
        .task { await load() }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            let shelf = try await Audiobooks(api: api).books(cursor: nil)
            books = shelf.books
            next = shelf.next
            playlists = (try? await Playlists(api: api).list()) ?? []
        } catch {
            books = books ?? []
            problem = "Audiobooks didn't load. Pull to try again."
        }
    }

    private func loadMore() async {
        guard let api = model.api, let cursor = next else { return }
        if let shelf = try? await Audiobooks(api: api).books(cursor: cursor) {
            books = (books ?? []) + shelf.books
            next = shelf.next
        }
    }

    private func add(_ playlist: Playlist) async {
        guard let api = model.api else { return }
        adding = playlist.id
        defer { adding = nil }
        do {
            try await Audiobooks(api: api).create(from: playlist.id, device: Player.device)
            problem = nil
            await load()
        } catch {
            problem = "Your audiobook request was not confirmed."
        }
    }
}

struct AudiobookView: View {
    @Environment(AppModel.self) private var model
    let route: AudiobookRoute
    @State private var book: Audiobooks.Book?
    @State private var failed = false
    @State private var label = ""
    @State private var saving = false
    @State private var problem: String?

    var current: (index: Int, position: Double)? {
        guard let book, let id = model.player?.current?.id, let index = book.chapters.firstIndex(where: { $0.track.id == id }) else { return nil }
        return (index, model.player?.position ?? 0)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header
                if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.warning) }
                if let book {
                    chapters(book)
                    bookmarks(book)
                } else if failed {
                    HStack(spacing: 8) {
                        Text("This audiobook didn't load.").foregroundStyle(Theme.soft)
                        TextAction(title: "Check audiobook") { Task { await load() } }
                    }
                    .font(Theme.small)
                } else {
                    Text("Checking audiobook…").font(Theme.small).foregroundStyle(Theme.soft)
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
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .center, spacing: 14) {
                Cover(path: route.playlist.artworkPath, size: 96, radius: 10, mark: "book")
                VStack(alignment: .leading, spacing: 4) {
                    Text("Audiobook").font(Theme.smallStrong).foregroundStyle(Theme.soft)
                    Text(book?.playlist.name ?? route.playlist.name).font(Theme.title).tracking(Theme.titleTracking).lineLimit(3).accessibilityAddTraits(.isHeader)
                    if let book { Text("\(book.chapters.count) \(book.chapters.count == 1 ? "chapter" : "chapters")").foregroundStyle(Theme.soft) }
                }
            }
            if let overall = book?.overall {
                VStack(alignment: .leading, spacing: 4) {
                    GeometryReader { proxy in
                        ZStack(alignment: .leading) {
                            Capsule().fill(Theme.rest)
                            Capsule().fill(Theme.text).frame(width: proxy.size.width * overall.fraction)
                        }
                    }
                    .frame(height: 6)
                    .accessibilityLabel("Position in the audiobook")
                    .accessibilityValue("\(Int((overall.fraction * 100).rounded())) percent")
                    Text("\(Int((overall.fraction * 100).rounded()))% · \(overall.left)").font(Theme.small).foregroundStyle(Theme.soft)
                }
            }
            if let book, !book.chapters.isEmpty {
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 8) { actions(book) }
                    VStack(alignment: .leading, spacing: 8) { actions(book) }
                }
            }
        }
        .padding(.top, 8)
    }

    @ViewBuilder private func actions(_ book: Audiobooks.Book) -> some View {
        if let label = book.continueLabel, let continuation = book.continuation {
            PlayAllButton(label: label) { play(book, at: continuation.index, from: continuation.positionMs) }
                .fixedSize()
        }
        Button("Play from the beginning") { play(book, at: 0, from: 0) }
            .buttonStyle(.plain)
            .font(Theme.bodyMedium)
            .lineLimit(1)
            .padding(.horizontal, 18)
            .frame(minHeight: 48)
            .overlay(Capsule().strokeBorder(Theme.line))
            .fixedSize()
    }

    private func chapters(_ book: Audiobooks.Book) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Chapters").font(Theme.section).accessibilityAddTraits(.isHeader).padding(.bottom, 4)
            Text("This audiobook follows the chapters in its playlist.").font(Theme.small).foregroundStyle(Theme.soft).padding(.bottom, 6)
            if book.chapters.isEmpty {
                Text("No chapters yet").font(Theme.small).foregroundStyle(Theme.soft)
            }
            ForEach(Array(book.chapters.enumerated()), id: \.element.id) { index, chapter in
                Button { play(book, at: index, from: 0) } label: {
                    HStack(spacing: 12) {
                        Text("\(index + 1)").font(Theme.smallStrong).foregroundStyle(Theme.quiet).frame(width: 28)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(chapter.track.title.isEmpty ? "Untitled chapter" : chapter.track.title)
                                .foregroundStyle(model.player?.current?.id == chapter.track.id ? Theme.focus : Theme.text)
                                .lineLimit(2)
                            Text(chapter.track.isPlayable ? SourceAttachment.clock(chapter.track.durationMs) : "Chapter unavailable")
                                .font(Theme.small)
                                .foregroundStyle(chapter.track.isPlayable ? Theme.quiet : Theme.warning)
                        }
                        Spacer(minLength: 0)
                    }
                    .frame(minHeight: 52)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func bookmarks(_ book: Audiobooks.Book) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Bookmarks").font(Theme.section).accessibilityAddTraits(.isHeader)
            if book.bookmarks.isEmpty {
                Text("No bookmarks yet.").font(Theme.small).foregroundStyle(Theme.soft)
            }
            ForEach(book.bookmarks) { mark in
                HStack(spacing: 8) {
                    Button { play(book, at: mark.chapterIndex, from: mark.positionMs) } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(mark.label).lineLimit(2)
                            Text("Ch. \(mark.chapterIndex + 1) · \(SourceAttachment.clock(mark.positionMs))").font(Theme.small).foregroundStyle(Theme.quiet)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .frame(minHeight: 48)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    IconButton(icon: "close", label: "Remove bookmark \(mark.label)", color: Theme.quiet) { Task { await remove(mark, from: book) } }
                        .disabled(saving)
                }
            }
            VStack(alignment: .leading, spacing: 8) {
                Text("Add a bookmark").font(Theme.bodyStrong)
                if let current {
                    Text("Ch. \(current.index + 1) · \(SourceAttachment.clock(Int(current.position * 1000)))").font(Theme.small).foregroundStyle(Theme.quiet)
                    HStack(spacing: 8) {
                        TextField("", text: $label, prompt: Text("Label").foregroundStyle(Theme.quiet))
                            .padding(.horizontal, 14)
                            .frame(minHeight: 44)
                            .background(Capsule().fill(Theme.raised))
                        Button(saving ? "Saving bookmark change…" : "Save") { Task { await addBookmark(book, at: current) } }
                            .buttonStyle(.plain)
                            .font(Theme.bodyStrong)
                            .foregroundStyle(Theme.focus)
                            .frame(minHeight: 44)
                            .disabled(saving || label.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                } else {
                    Text("Bookmarks need a chapter that is available to play. Play a chapter of this audiobook first.").font(Theme.small).foregroundStyle(Theme.soft)
                }
            }
            .padding(.top, 6)
        }
    }

    private func play(_ book: Audiobooks.Book, at index: Int, from positionMs: Int) {
        model.player?.playAudiobook(book.chapters.map(\.track), anchors: book.anchors, at: index, from: book.playlist.name, startingAt: Double(positionMs) / 1000)
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            book = try await Audiobooks(api: api).detail(route.playlist.id)
            failed = false
        } catch {
            failed = book == nil
        }
    }

    private func addBookmark(_ book: Audiobooks.Book, at current: (index: Int, position: Double)) async {
        guard let api = model.api else { return }
        saving = true
        defer { saving = false }
        do {
            _ = try await Audiobooks(api: api).bookmark(book.anchors[current.index], at: Int(current.position * 1000), label: label.trimmingCharacters(in: .whitespaces), device: Player.device)
            label = ""
            problem = nil
            await load()
        } catch {
            problem = "A bookmark change is in progress or needs a retry."
        }
    }

    private func remove(_ mark: Audiobooks.Bookmark, from book: Audiobooks.Book) async {
        guard let api = model.api else { return }
        saving = true
        defer { saving = false }
        do {
            try await Audiobooks(api: api).removeBookmark(mark.id, from: book.playlist.id, device: Player.device)
            problem = nil
            await load()
        } catch {
            problem = "A bookmark change is in progress or needs a retry."
        }
    }
}
