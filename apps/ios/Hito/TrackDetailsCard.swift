import HitoKit
import SwiftUI

struct TrackDetailsCard: View {
    @Environment(AppModel.self) private var model
    let track: Track
    @Binding var details: TrackMetadata.Details?
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Your track details").font(Theme.section).accessibilityAddTraits(.isHeader)
            Text("Changes apply to your account across this recording's albums and playlists.")
                .font(Theme.small).foregroundStyle(Theme.soft)
            if let details {
                MetadataFieldEditor(field: .title, label: "Track title", value: details.title) { await save(.title, $0) }
                MetadataFieldEditor(field: .artist, label: "Track artist", value: details.artist) { await save(.artist, $0) }
                if let credits = details.credits {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Credits").font(Theme.label).foregroundStyle(Theme.quiet)
                        Text(credits).font(Theme.small)
                    }
                }
                if !details.facts.isEmpty {
                    Text(details.facts.joined(separator: " · ")).font(Theme.small).foregroundStyle(Theme.quiet)
                }
                if let id = details.musicBrainz {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("MusicBrainz recording").font(Theme.label).foregroundStyle(Theme.quiet)
                        Text(id).font(Theme.caption).foregroundStyle(Theme.soft).textSelection(.enabled)
                    }
                }
            } else if problem == nil {
                ProgressView().tint(Theme.soft)
            }
            if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.warning) }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
        .task { await load() }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            details = try await TrackMetadata(api: api, device: Player.device).load(track.id)
            problem = nil
        } catch {
            problem = "Track details couldn't load. Pull to try again."
        }
    }

    private func save(_ field: TrackMetadata.Field, _ personal: String?) async -> String? {
        guard let api = model.api else { return nil }
        do {
            try await TrackMetadata(api: api, device: Player.device).save(track.id, field: field, personal: personal)
            await load()
            return nil
        } catch {
            return "The change wasn't saved. Try again."
        }
    }
}

struct MetadataFieldEditor: View {
    let field: TrackMetadata.Field
    let label: String
    let value: TrackMetadata.Value
    let save: (String?) async -> String?
    @State private var draft = ""
    @State private var saving = false
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(Theme.label).foregroundStyle(Theme.quiet)
            TextField(label, text: $draft)
                .padding(.horizontal, 14)
                .frame(minHeight: 44)
                .background(RoundedRectangle(cornerRadius: 10).fill(Theme.raised))
                .disabled(saving)
            Text(value.note(field: field.rawValue)).font(Theme.caption).foregroundStyle(Theme.quiet)
            if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.warning) }
            HStack(spacing: 8) {
                if value.personal != nil {
                    Button("Use source \(field.rawValue)") { Task { await commit(nil) } }
                        .buttonStyle(.plain)
                        .font(Theme.smallStrong)
                        .padding(.horizontal, 14)
                        .frame(minHeight: 40)
                        .overlay(Capsule().strokeBorder(Theme.line))
                        .disabled(saving)
                }
                let trimmed = draft.trimmingCharacters(in: .whitespaces)
                Button(saving ? "Saving…" : "Save personal \(field.rawValue)") { Task { await commit(trimmed) } }
                    .buttonStyle(.plain)
                    .font(Theme.smallStrong)
                    .foregroundStyle(Theme.onAccent)
                    .padding(.horizontal, 14)
                    .frame(minHeight: 40)
                    .background(Capsule().fill(Theme.accent))
                    .opacity(saving || trimmed.isEmpty || trimmed == value.personal || trimmed == value.value ? 0.4 : 1)
                    .disabled(saving || trimmed.isEmpty || trimmed == value.personal || trimmed == value.value)
            }
        }
        .onAppear { draft = value.value }
        .onChange(of: value) { draft = value.value }
    }

    private func commit(_ personal: String?) async {
        saving = true
        problem = await save(personal)
        saving = false
    }
}
