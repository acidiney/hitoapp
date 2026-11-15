import HitoKit
import SwiftUI

struct ReviewQueueButton: View {
    @Environment(AppModel.self) private var model
    let changed: () -> Void
    @State private var waiting: Int?
    @State private var dismissed = 0
    @State private var open = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let waiting, waiting + dismissed > 0 {
                Button { open = true } label: {
                    Text(waiting > 0 ? "\(waiting) \(waiting == 1 ? "artist needs" : "artists need") review" : "Dismissed suggestions (\(dismissed))")
                        .font(Theme.smallStrong)
                        .foregroundStyle(Theme.text)
                        .padding(.horizontal, 16)
                        .frame(minHeight: 44)
                        .overlay(Capsule().strokeBorder(Theme.line))
                        .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .padding(.vertical, 8)
            }
        }
        .task { await count() }
        .sheet(isPresented: $open, onDismiss: { Task { await count() } }) {
            ReviewQueueView(changed: changed).environment(model)
        }
    }

    private func count() async {
        guard let api = model.api else { return }
        let queue = ReviewQueue(api: api)
        waiting = (try? await queue.waiting(offset: 0))?.total
        dismissed = (try? await queue.dismissed(offset: 0))?.total ?? 0
    }
}

struct ReviewListing {
    var items: [ReviewItem]
    var total: Int

    init(_ page: Page<ReviewItem>) {
        items = page.items
        total = page.total
    }
}

struct ReviewQueueView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var close
    let changed: () -> Void
    @State private var waiting: ReviewListing?
    @State private var dismissed: ReviewListing?
    @State private var pending: String?
    @State private var problem: String?
    @State private var failed = false
    @State private var searching: ReviewItem?

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    Text("Hito could not decide these credits on its own. Confirming an artist applies it to the tracks of that credit that have no artist yet.")
                        .font(Theme.small).foregroundStyle(Theme.soft)
                    if let problem {
                        Text(problem).font(Theme.small).foregroundStyle(Theme.warning)
                    }
                    if failed {
                        HStack(spacing: 8) {
                            Text("The review queue didn't load.").foregroundStyle(Theme.soft)
                            TextAction(title: "Try again") { Task { await load() } }
                        }
                        .font(Theme.small)
                    } else if let waiting {
                        ForEach(waiting.items) { item in waitingRow(item) }
                        if waiting.items.count < waiting.total {
                            TextAction(title: "Show more") { Task { await more(dismissed: false) } }
                        }
                    } else {
                        ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 80)
                    }
                    if let dismissed, dismissed.total > 0 {
                        dismissedSection(dismissed)
                    }
                }
                .padding(16)
            }
            .navigationTitle("Artists to review")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Close") { close() } }
            }
        }
        .task { await load() }
        .fullScreenCover(item: $searching) { item in
            if let recording = item.searchRecordingId {
                ArtistSheet(recording: recording, credit: item.name) {
                    changed()
                    Task { await load() }
                }
                .presentationBackground(.clear)
            }
        }
    }

    private func credit(_ item: ReviewItem) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(item.name).font(Theme.bodyMedium)
            Text(item.tracks).font(Theme.small).foregroundStyle(Theme.quiet)
        }
    }

    private func waitingRow(_ item: ReviewItem) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                credit(item)
                Spacer(minLength: 8)
                if pending == item.id { ProgressView().tint(Theme.soft).controlSize(.small) }
            }
            if item.needsSearch {
                HStack(spacing: 8) {
                    Text("No match found automatically").font(Theme.small).foregroundStyle(Theme.soft)
                    if item.searchRecordingId != nil {
                        TextAction(title: "Search") { searching = item }
                            .accessibilityLabel("Search for \(item.name)")
                    }
                }
            }
            ForEach(item.candidates) { candidate in
                candidateRow(candidate, action: "Confirm") {
                    await decide(item) { try await $0.confirm(credit: item.id, artist: candidate.providerArtistId) }
                }
                .accessibilityLabel("Confirm \(candidate.label), for \(item.name)")
            }
            if !item.needsSearch {
                TextAction(title: "None of these") {
                    Task { await decide(item) { try await $0.dismiss(credit: item.id, artist: nil) } }
                }
                .disabled(pending != nil)
                .accessibilityLabel("None of these for \(item.name)")
            }
        }
        .padding(.vertical, 8)
        .overlay(alignment: .bottom) { Rectangle().fill(Theme.line).frame(height: 1) }
    }

    private func dismissedSection(_ page: ReviewListing) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Dismissed suggestions (\(page.total))").font(Theme.section).accessibilityAddTraits(.isHeader)
            Text("You said these were not the right artist. Restoring offers them again for confirmation.")
                .font(Theme.small).foregroundStyle(Theme.soft)
            ForEach(page.items) { item in
                VStack(alignment: .leading, spacing: 8) {
                    credit(item)
                    ForEach(item.candidates) { candidate in
                        candidateRow(candidate, action: "Restore") {
                            await decide(item) { try await $0.restore(credit: item.id, artist: candidate.providerArtistId) }
                        }
                        .accessibilityLabel("Restore \(candidate.label), for \(item.name)")
                    }
                    if item.candidates.count > 1 {
                        TextAction(title: "Restore all") {
                            Task { await decide(item) { try await $0.restore(credit: item.id, artist: nil) } }
                        }
                        .disabled(pending != nil)
                    }
                }
                .padding(.vertical, 8)
            }
            if page.items.count < page.total {
                TextAction(title: "Show more") { Task { await more(dismissed: true) } }
            }
        }
        .padding(.top, 16)
    }

    private func candidateRow(_ candidate: ReviewCandidate, action: String, run: @escaping () async -> Void) -> some View {
        HStack(alignment: .center, spacing: 12) {
            Text(candidate.details.isEmpty ? candidate.name : candidate.details)
                .font(Theme.small).foregroundStyle(Theme.text)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button { Task { await run() } } label: {
                Text(action).font(Theme.smallStrong).foregroundStyle(Theme.text)
                    .padding(.horizontal, 14).frame(minHeight: 44)
                    .overlay(Capsule().strokeBorder(Theme.line)).contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .disabled(pending != nil)
        }
        .accessibilityElement(children: .combine)
    }

    private func load() async {
        guard let api = model.api else { return }
        let queue = ReviewQueue(api: api)
        do {
            waiting = ReviewListing(try await queue.waiting(offset: 0))
            failed = false
        } catch is CancellationError {
            return
        } catch {
            failed = true
            model.handle(error)
        }
        dismissed = (try? await queue.dismissed(offset: 0)).map(ReviewListing.init)
    }

    private func more(dismissed list: Bool) async {
        guard let api = model.api, let current = list ? dismissed : waiting else { return }
        let queue = ReviewQueue(api: api)
        guard let next = try? await (list ? queue.dismissed(offset: current.items.count) : queue.waiting(offset: current.items.count)) else { return }
        var joined = current
        joined.items += next.items
        joined.total = next.total
        if list { dismissed = joined } else { waiting = joined }
    }

    private func decide(_ item: ReviewItem, _ save: (ReviewQueue) async throws -> Void) async {
        guard let api = model.api else { return }
        pending = item.id
        problem = nil
        defer { pending = nil }
        do {
            try await save(ReviewQueue(api: api))
            changed()
        } catch is CancellationError {
            return
        } catch {
            problem = "This decision could not be saved. Try again."
            model.handle(error)
        }
        await load()
    }
}
