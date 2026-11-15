import HitoKit
import SwiftUI

struct SourceHistoryCard: View {
    @Environment(AppModel.self) private var model
    let track: Track
    @State private var page: MetadataHistory.Page?
    @State private var older = false
    @State private var busy = false
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Where these details came from").font(Theme.section).accessibilityAddTraits(.isHeader)
            if older {
                TextAction(title: "Latest observations") { Task { await load(before: nil) } }
            }
            if let page {
                if page.observations.isEmpty {
                    Text("No source observations have been recorded yet.").font(Theme.small).foregroundStyle(Theme.soft)
                }
                ForEach(page.observations) { observation in
                    ObservationRow(observation: observation, selected: observation.id == page.selected)
                }
                if let next = page.next {
                    TextAction(title: busy ? "Loading…" : "Older observations") { Task { await load(before: next) } }
                        .disabled(busy)
                }
            } else if problem == nil {
                ProgressView().tint(Theme.soft)
            }
            if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.warning) }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
        .task { await load(before: nil) }
    }

    private func load(before: Int?) async {
        guard let api = model.api else { return }
        busy = true
        defer { busy = false }
        do {
            page = try await MetadataHistory(api: api).page(track.id, before: before)
            older = before != nil
            problem = nil
        } catch {
            problem = "The source history didn't load. Pull to try again."
        }
    }
}

struct ObservationRow: View {
    let observation: MetadataHistory.Observation
    let selected: Bool
    @State private var showingTags = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(observation.when.map { $0.formatted(date: .abbreviated, time: .shortened) } ?? "Imported during upgrade")
                .font(Theme.caption).foregroundStyle(Theme.quiet)
            Text(observation.heading).font(Theme.bodyStrong)
            Text(observation.summary).font(Theme.small).foregroundStyle(Theme.soft)
            if selected {
                Text("Current metadata").font(Theme.smallStrong).foregroundStyle(Theme.focus)
            }
            switch observation.origin {
            case .legacy:
                Text("Original tag details were not recorded. The next successful scan can inspect them.").font(Theme.small).foregroundStyle(Theme.quiet)
            case let .provider(recording, revision):
                if let recording { Text("Provider recording · \(recording)").font(Theme.small).foregroundStyle(Theme.quiet) }
                if let revision { Text("Provider revision · \(revision)").font(Theme.small).foregroundStyle(Theme.quiet) }
            case .file:
                DisclosureGroup("Inspected tags", isExpanded: $showingTags) {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(observation.tags, id: \.label) { tag in
                            HStack(alignment: .firstTextBaseline) {
                                Text(tag.label).font(Theme.small).foregroundStyle(Theme.quiet).frame(width: 96, alignment: .leading)
                                if let value = tag.value {
                                    Text(value).font(Theme.small)
                                    if let place = tag.place { Text(place).font(Theme.caption).foregroundStyle(Theme.quiet) }
                                } else {
                                    Text("No tag").font(Theme.small).foregroundStyle(Theme.quiet)
                                }
                            }
                        }
                    }
                    .padding(.top, 4)
                }
                .font(Theme.smallStrong)
                .tint(Theme.focus)
            }
        }
        .padding(.vertical, 8)
        .overlay(alignment: .bottom) { Theme.barLine.frame(height: 1) }
    }
}
