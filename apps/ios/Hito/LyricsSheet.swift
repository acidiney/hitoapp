import HitoKit
import SwiftUI

struct LyricsSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let track: Track
    let changed: () -> Void
    @State private var choices: [LyricChoices.Identity]?
    @State private var services: [LyricChoices.Service]?
    @State private var service: LyricChoices.Service?
    @State private var query = ""
    @State private var matches: [LyricChoices.Match] = []
    @State private var searched = false
    @State private var busy = false
    @State private var removing: String?
    @State private var problem: String?
    @State private var reviewing: LyricChoices.Review?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text("Lyrics for this track").font(Theme.pageTitle).accessibilityAddTraits(.isHeader)
                    Spacer()
                    IconButton(icon: "close", label: "Close") { dismiss() }
                }
                Text("Choose where the lyrics come from, remove a choice you no longer want, or find a different match.")
                    .font(Theme.small).foregroundStyle(Theme.soft)
                if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.warning) }
                if let choices, !choices.isEmpty {
                    Text("Lyrics from").font(Theme.label).foregroundStyle(Theme.quiet)
                    ForEach(choices) { choice in
                        HStack(spacing: 12) {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(choice.label).font(Theme.bodyStrong)
                                Text(choice.origin).font(Theme.small).foregroundStyle(Theme.quiet)
                            }
                            Spacer()
                            if let id = choice.identityId {
                                Button(removing == id ? "Removing…" : "Remove") { Task { await remove(id) } }
                                    .buttonStyle(.plain)
                                    .font(Theme.smallStrong)
                                    .foregroundStyle(Theme.focus)
                                    .frame(minHeight: 44)
                                    .disabled(removing != nil)
                                    .accessibilityLabel("Remove \(choice.label)")
                            }
                        }
                    }
                }
                finder
            }
            .padding(20)
        }
        .background(Theme.field.ignoresSafeArea())
        .foregroundStyle(Theme.text)
        .task { await load() }
        .sheet(item: Binding(get: { reviewing.map { ReviewItem(review: $0) } }, set: { reviewing = $0?.review })) { item in
            LyricReviewSheet(track: track, service: service?.label ?? "", review: item.review, busy: busy) { Task { await keep(item.review) } }
        }
    }

    struct ReviewItem: Identifiable {
        let review: LyricChoices.Review
        var id: String { review.stamp }
    }

    @ViewBuilder private var finder: some View {
        Text("Find lyrics").font(Theme.section).accessibilityAddTraits(.isHeader)
        if services?.isEmpty == true {
            Text("No connected service offers lyrics. Connect a lyrics service in Plugins.").font(Theme.small).foregroundStyle(Theme.soft)
        } else if let services {
            if services.count > 1 {
                Picker("Lyrics service", selection: Binding(get: { service?.id ?? "" }, set: { id in service = services.first { $0.id == id }; matches = []; searched = false })) {
                    ForEach(services) { Text($0.label).tag($0.id) }
                }
                .pickerStyle(.segmented)
            }
            HStack(spacing: 8) {
                TextField("Track and artist", text: $query)
                    .submitLabel(.search)
                    .onSubmit { Task { await search() } }
                    .padding(.horizontal, 14)
                    .frame(minHeight: 44)
                    .background(Capsule().fill(Theme.raised))
                Button(busy && reviewing == nil ? "Searching…" : "Search") { Task { await search() } }
                    .buttonStyle(.plain)
                    .font(Theme.bodyMedium)
                    .foregroundStyle(Theme.focus)
                    .frame(minHeight: 44)
                    .disabled(busy || query.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            ForEach(matches) { match in
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(match.title).font(Theme.bodyStrong).lineLimit(2)
                        Text([SourceAttachment.artist(match.artist), match.album, match.durationMs.map(SourceAttachment.clock)].compactMap { $0 }.joined(separator: " · "))
                            .font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(2)
                    }
                    Spacer(minLength: 8)
                    Button("Review") { Task { await review(match) } }
                        .buttonStyle(.plain)
                        .font(Theme.smallStrong)
                        .foregroundStyle(Theme.focus)
                        .frame(minHeight: 44)
                        .disabled(busy)
                }
            }
            if searched && matches.isEmpty && !busy {
                Text("No lyric matches. Try another track or artist.").font(Theme.small).foregroundStyle(Theme.soft)
            }
        } else if problem == nil {
            ProgressView().tint(Theme.soft)
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        let lyrics = LyricChoices(api: api)
        do {
            choices = try await lyrics.identities(track.id)
            if services == nil {
                services = try await lyrics.services(track.id)
                service = services?.first
                query = "\(track.title) \(track.artist)"
            }
        } catch is CancellationError {
        } catch {
            problem = PluginTrouble.message(error, otherwise: "Lyric services didn't load. Close this and try again.")
            model.handle(error)
        }
    }

    private func search() async {
        guard let api = model.api, let service else { return }
        busy = true
        defer { busy = false }
        do {
            matches = try await LyricChoices(api: api).search(track.id, service, query: query.trimmingCharacters(in: .whitespaces))
            searched = true
            problem = nil
        } catch {
            matches = []
            problem = PluginTrouble.message(error, otherwise: "\(service.label) didn't answer. Try again.")
        }
    }

    private func review(_ match: LyricChoices.Match) async {
        guard let api = model.api, let service else { return }
        busy = true
        defer { busy = false }
        do {
            reviewing = try await LyricChoices(api: api).review(track.id, service, match)
            problem = nil
        } catch is CancellationError {
        } catch {
            problem = PluginTrouble.message(error, otherwise: "This match couldn't be checked. Try another one.")
        }
    }

    private func keep(_ review: LyricChoices.Review) async {
        guard let api = model.api, let service else { return }
        busy = true
        defer { busy = false }
        do {
            _ = try await LyricChoices(api: api).keep(track.id, service, review.match, stamp: review.stamp)
            reviewing = nil
            matches = []
            searched = false
            problem = nil
            await load()
            changed()
        } catch {
            reviewing = nil
            problem = PluginTrouble.message(error, otherwise: "These lyrics weren't saved. Try again.")
        }
    }

    private func remove(_ identity: String) async {
        guard let api = model.api else { return }
        removing = identity
        defer { removing = nil }
        do {
            try await LyricChoices(api: api).remove(track.id, identity: identity)
            problem = nil
            await load()
            changed()
        } catch {
            problem = "The choice is still there. Try again."
        }
    }
}

struct LyricReviewSheet: View {
    @Environment(\.dismiss) private var dismiss
    let track: Track
    let service: String
    let review: LyricChoices.Review
    let busy: Bool
    let confirm: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Are these the lyrics you want?").font(Theme.pageTitle).accessibilityAddTraits(.isHeader)
                Spacer()
                IconButton(icon: "close", label: "Close") { dismiss() }
            }
            Text("Check that this \(service) match is the same track. Nothing is saved until you choose.")
                .font(Theme.small).foregroundStyle(Theme.soft)
            Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 10) {
                GridRow {
                    Text("").font(Theme.label)
                    Text("Saved track").font(Theme.label).foregroundStyle(Theme.quiet)
                    Text("Match from \(service)").font(Theme.label).foregroundStyle(Theme.quiet)
                }
                row("Title", track.title, review.match.title)
                row("Artist", track.artist, SourceAttachment.artist(review.match.artist) ?? "Artist not listed")
                row("Album", track.album ?? "—", review.match.album ?? "Album not listed")
                row("Length", SourceAttachment.clock(track.durationMs), review.match.durationMs.map(SourceAttachment.clock) ?? "Length not listed")
            }
            HStack(spacing: 8) {
                Button("Cancel") { dismiss() }
                    .buttonStyle(.plain)
                    .font(Theme.bodyMedium)
                    .padding(.horizontal, 18)
                    .frame(minHeight: 48)
                    .overlay(Capsule().strokeBorder(Theme.line))
                PlayAllButton(label: busy ? "Saving…" : "Use these lyrics", action: confirm)
                    .disabled(busy)
            }
            Spacer()
        }
        .padding(20)
        .background(Theme.field.ignoresSafeArea())
        .foregroundStyle(Theme.text)
        .presentationDetents([.medium, .large])
    }

    private func row(_ name: String, _ mine: String, _ theirs: String) -> some View {
        GridRow {
            Text(name).font(Theme.smallStrong).foregroundStyle(Theme.quiet)
            Text(mine).font(Theme.small).lineLimit(3)
            Text(theirs).font(Theme.small).lineLimit(3)
        }
    }
}
