import HitoKit
import SwiftUI

struct CopiesCard: View {
    @Environment(AppModel.self) private var model
    let track: Track
    @State private var copies: [RecordingCopies.Copy] = []
    @State private var separating: String?
    @State private var problem: String?

    var body: some View {
        Group {
            if !copies.isEmpty || problem != nil {
                VStack(alignment: .leading, spacing: 12) {
                    Text("Copies of this song").font(Theme.section).accessibilityAddTraits(.isHeader)
                    if !copies.isEmpty {
                        Text("Hito can prove these are the same song, so your lists show one card. Every copy keeps its own audio, likes and playlists.")
                            .font(Theme.small).foregroundStyle(Theme.soft)
                    }
                    if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.warning) }
                    ForEach(copies) { copy in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(copy.title).font(copy.id == track.id ? Theme.bodyStrong : Theme.body)
                            Text(copy.line).font(Theme.small).foregroundStyle(Theme.quiet)
                            Text("\(copy.audio) · \(copy.proof)").font(Theme.caption).foregroundStyle(copy.available ? Theme.soft : Theme.warning)
                            ForEach(copy.sources, id: \.self) { Text($0).font(Theme.caption).foregroundStyle(Theme.quiet) }
                            if copy.canSeparate {
                                Button(separating == copy.id ? "Separating…" : "These are different songs") { Task { await separate(copy.id) } }
                                    .buttonStyle(.plain)
                                    .font(Theme.smallStrong)
                                    .padding(.horizontal, 14)
                                    .frame(minHeight: 40)
                                    .overlay(Capsule().strokeBorder(Theme.line))
                                    .disabled(separating != nil)
                            }
                        }
                        .padding(.vertical, 6)
                    }
                }
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
            }
        }
        .task { await load() }
    }

    private func load() async {
        guard let api = model.api else { return }
        copies = (try? await RecordingCopies(api: api).load(track.id)) ?? []
    }

    private func separate(_ copy: String) async {
        guard let api = model.api else { return }
        separating = copy
        defer { separating = nil }
        do {
            _ = try await RecordingCopies(api: api).separate(copy)
            problem = nil
            await load()
        } catch {
            problem = "The copies weren't separated. Try again."
        }
    }
}
