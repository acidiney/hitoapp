import HitoKit
import SwiftUI

struct FindAudioCard: View {
    @Environment(AppModel.self) private var model
    let track: Track
    let prominent: Bool
    let attached: () -> Void
    @State private var services: [PluginConnection]?
    @State private var service: PluginConnection?
    @State private var query = ""
    @State private var results: [PluginResult] = []
    @State private var searched = false
    @State private var busy = false
    @State private var problem: String?
    @State private var reviewing: Reviewing?
    @State private var done = false

    struct Reviewing: Identifiable {
        let candidate: SourceAttachment.Candidate
        let review: SourceAttachment.Review
        var id: String { candidate.providerRecordingId }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(prominent ? "Find audio" : "Add another source").font(Theme.section).accessibilityAddTraits(.isHeader)
            Text("Find audio through your connected music services and review the match before adding it to this saved track.")
                .font(Theme.small).foregroundStyle(Theme.soft)
            if services == nil {
                PlayAllButton(label: busy ? "Loading services…" : "Find a source") { Task { await open() } }
                    .disabled(busy)
            } else if services?.isEmpty == true {
                Text("No connected service offers both music search and audio sources. Connect a music service in Plugins.")
                    .font(Theme.small).foregroundStyle(Theme.soft)
            } else {
                search
            }
            if services == nil || services?.isEmpty == true, let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.warning) }
            if done { Text("Source added to this track.").font(Theme.small).foregroundStyle(Theme.focus) }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
        .sheet(item: $reviewing) { item in
            ReviewMatchSheet(track: track, service: service?.name ?? "", review: item.review, busy: busy) {
                Task { await add(item) }
            }
        }
    }

    @ViewBuilder private var search: some View {
        if let services, services.count > 1 {
            Picker("Service", selection: Binding(get: { service?.id ?? "" }, set: { id in service = services.first { $0.id == id }; results = []; searched = false })) {
                ForEach(services) { Text($0.name).tag($0.id) }
            }
            .pickerStyle(.segmented)
        }
        HStack(spacing: 8) {
            TextField("Track or artist", text: $query)
                .textInputAutocapitalization(.never)
                .submitLabel(.search)
                .onSubmit { Task { await find() } }
                .padding(.horizontal, 14)
                .frame(minHeight: 44)
                .background(Capsule().fill(Theme.raised))
            Button("Search") { Task { await find() } }
                .buttonStyle(.plain)
                .font(Theme.bodyMedium)
                .foregroundStyle(Theme.focus)
                .frame(minHeight: 44)
                .disabled(busy || query.trimmingCharacters(in: .whitespaces).isEmpty)
        }
        if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.warning) }
        if busy && reviewing == nil { ProgressView().tint(Theme.soft) }
        ForEach(results) { hit in
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(hit.title).font(Theme.bodyStrong).lineLimit(2)
                    let facts = [SourceAttachment.artist(hit.artist), hit.durationMs.map(SourceAttachment.clock), service?.name].compactMap { $0 }
                    Text(facts.joined(separator: " · ")).font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(2)
                    if sameTrack(hit) {
                        Text("Same length and artist").font(Theme.caption).foregroundStyle(Theme.focus)
                    }
                }
                Spacer(minLength: 8)
                Button("Review match") { Task { await review(hit) } }
                    .buttonStyle(.plain)
                    .font(Theme.smallStrong)
                    .foregroundStyle(Theme.focus)
                    .frame(minHeight: 44)
                    .disabled(busy)
            }
        }
        if searched && results.isEmpty && !busy && problem == nil {
            Text("No matching tracks. Try another track or artist.").font(Theme.small).foregroundStyle(Theme.soft)
        }
    }

    private func sameTrack(_ hit: PluginResult) -> Bool {
        SourceAttachment.compare(saved: track.durationMs, candidate: hit.durationMs)?.close == true
            && SourceAttachment.artist(hit.artist)?.lowercased() == track.artist.trimmingCharacters(in: .whitespaces).lowercased()
    }

    private func open() async {
        guard let api = model.api else { return }
        busy = true
        defer { busy = false }
        do {
            let found = try await Plugins(api: api).load().connections.filter { $0.capabilities.contains("search") && $0.capabilities.contains("sources") }
            services = found
            service = found.first
            query = "\(track.title) \(track.artist)"
            problem = nil
        } catch {
            problem = "Your services couldn't load. Try again."
        }
    }

    private func find() async {
        guard let api = model.api, let service else { return }
        busy = true
        defer { busy = false }
        do {
            results = try await Plugins(api: api).search(service.id, revision: service.revision, query: query.trimmingCharacters(in: .whitespaces))
            searched = true
            problem = nil
        } catch {
            results = []
            problem = "\(service.name) didn't answer. Try again."
        }
    }

    private func review(_ hit: PluginResult) async {
        guard let api = model.api, let service else { return }
        busy = true
        defer { busy = false }
        let candidate = SourceAttachment.Candidate(installationId: service.id, installationRevision: service.revision, providerRecordingId: hit.id)
        do {
            reviewing = Reviewing(candidate: candidate, review: try await SourceAttachment(api: api).review(track.id, candidate))
            problem = nil
        } catch let refused as SourceAttachment.Problem {
            problem = refused.message
        } catch {
            problem = "This match couldn't be reviewed. Try another one."
        }
    }

    private func add(_ item: Reviewing) async {
        guard let api = model.api else { return }
        busy = true
        defer { busy = false }
        do {
            _ = try await SourceAttachment(api: api).attach(track.id, item.candidate, stamp: item.review.stamp)
            reviewing = nil
            results = []
            done = true
            problem = nil
            attached()
        } catch let refused as SourceAttachment.Problem {
            reviewing = nil
            problem = refused.message
        } catch {
            reviewing = nil
            problem = "This source could not be added to the saved track."
        }
    }
}

struct ReviewMatchSheet: View {
    @Environment(\.dismiss) private var dismiss
    let track: Track
    let service: String
    let review: SourceAttachment.Review
    let busy: Bool
    let confirm: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Review match").font(Theme.pageTitle).accessibilityAddTraits(.isHeader)
                Spacer()
                IconButton(icon: "close", label: "Close") { dismiss() }
            }
            Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 10) {
                GridRow {
                    Text("").font(Theme.label)
                    Text("Your track").font(Theme.label).foregroundStyle(Theme.quiet)
                    Text(service.isEmpty ? "Match" : service).font(Theme.label).foregroundStyle(Theme.quiet)
                }
                row("Title", track.title, review.title)
                row("Artist", track.artist, review.artist ?? "Unknown")
                row("Length", SourceAttachment.clock(track.durationMs), review.durationMs.map(SourceAttachment.clock) ?? "Unknown")
                if let album = review.album { row("Album", track.album ?? "—", album) }
            }
            if let comparison = SourceAttachment.compare(saved: track.durationMs, candidate: review.durationMs) {
                Text(comparison.text).font(Theme.small).foregroundStyle(comparison.close ? Theme.focus : Theme.warning)
            }
            Text("Adding this source lets Hito play this saved track from \(service.isEmpty ? "the service" : service). Your track's details stay as they are.")
                .font(Theme.small).foregroundStyle(Theme.soft)
            HStack(spacing: 8) {
                Button("Cancel") { dismiss() }
                    .buttonStyle(.plain)
                    .font(Theme.bodyMedium)
                    .padding(.horizontal, 18)
                    .frame(minHeight: 48)
                    .overlay(Capsule().strokeBorder(Theme.line))
                PlayAllButton(label: busy ? "Adding…" : "Add source", action: confirm)
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
