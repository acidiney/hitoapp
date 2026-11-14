import HitoKit
import SwiftUI

struct TrackView: View {
    @Environment(AppModel.self) private var model
    let track: Track
    @State private var choice: TrackSources.Choice?
    @State private var staged: String?
    @State private var saving = false
    @State private var problem: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header
                sources
                FindAudioCard(track: track, prominent: !(choice?.entries.contains(where: \.ready) ?? track.isPlayable)) {
                    Task { await load() }
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
        HStack(alignment: .top, spacing: 14) {
            Cover(path: track.artworkPath, size: 96, radius: 10)
            VStack(alignment: .leading, spacing: 4) {
                Text(track.title).font(Theme.title).lineLimit(3).accessibilityAddTraits(.isHeader)
                Text(track.artist).foregroundStyle(Theme.soft).lineLimit(2)
                HStack(spacing: 6) {
                    StatusDot(track: track)
                    Text(track.stateLabel).font(Theme.small).foregroundStyle(Theme.soft)
                }
                HStack(spacing: 8) {
                    PlayAllButton(label: "Play") { model.player?.play([track], at: 0, from: track.title) }
                    if let player = model.player { LikeButton(player: player, track: track) }
                }
                .padding(.top, 6)
            }
            Spacer(minLength: 0)
        }
    }

    @ViewBuilder private var sources: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                Text("Playback source").font(Theme.section).accessibilityAddTraits(.isHeader)
                Spacer()
                if let choice { Text(choice.modeLabel).font(Theme.small).foregroundStyle(Theme.quiet) }
            }
            if saving {
                Text("Saving source choice…").font(Theme.small).foregroundStyle(Theme.soft)
            }
            if let problem {
                Text(problem).font(Theme.small).foregroundStyle(Theme.warning)
            }
            if let choice {
                if choice.entries.isEmpty {
                    Text("No sources yet. Hito looks for one when the track plays.").font(Theme.small).foregroundStyle(Theme.soft)
                }
                VStack(spacing: 0) {
                    ForEach(choice.entries) { entry in
                        SourceRow(entry: entry, chosen: chosen(in: choice) == entry.id) { staged = entry.id }
                            .disabled(saving || !entry.selectable)
                    }
                }
                if choice.mode != .automatic || choice.entries.count > 1 || staged != nil {
                    HStack(spacing: 8) {
                        if choice.mode != .automatic {
                            Button("Return to automatic") { Task { await reset(choice) } }
                                .buttonStyle(.plain)
                                .font(Theme.bodyMedium)
                                .padding(.horizontal, 18)
                                .frame(minHeight: 44)
                                .overlay(Capsule().strokeBorder(Theme.line))
                                .disabled(saving)
                        }
                        PlayAllButton(label: "Use source") { Task { await use() } }
                            .opacity(canUse(choice) ? 1 : 0.4)
                            .disabled(!canUse(choice))
                    }
                }
                if let staged, !choice.entries.contains(where: { $0.id == staged && $0.selected }) {
                    Text("Selected, not saved yet. Use source to play from it.").font(Theme.small).foregroundStyle(Theme.soft)
                } else if choice.mode == .automatic, choice.entries.count == 1 {
                    Text("Automatic picks the best ready source. With one source there is nothing to choose.").font(Theme.small).foregroundStyle(Theme.soft)
                }
            } else if problem == nil {
                ProgressView().tint(Theme.soft)
            }
        }
        .padding(14)
        .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
    }

    private func chosen(in choice: TrackSources.Choice) -> String? {
        staged ?? (choice.mode == .manual ? choice.entries.first(where: \.selected)?.id : nil)
    }

    private func canUse(_ choice: TrackSources.Choice) -> Bool {
        guard !saving, let staged else { return false }
        return !choice.entries.contains { $0.id == staged && $0.selected && choice.mode == .manual }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            choice = try await TrackSources(api: api).load(track.id)
            problem = nil
        } catch {
            problem = "Sources couldn't load. Pull to try again."
        }
    }

    private func use() async {
        guard let api = model.api, let staged else { return }
        saving = true
        defer { saving = false }
        do {
            _ = try await TrackSources(api: api).choose(track.id, source: staged)
            self.staged = nil
            problem = nil
        } catch TrackSources.Problem.changed {
            problem = "This track's sources changed. Showing the latest."
        } catch {
            problem = "The source choice wasn't saved. Try again."
        }
        await load()
    }

    private func reset(_ current: TrackSources.Choice) async {
        guard let api = model.api else { return }
        saving = true
        defer { saving = false }
        do {
            _ = try await TrackSources(api: api).reset(track.id, revision: current.revision)
            staged = nil
            problem = nil
        } catch TrackSources.Problem.changed {
            problem = "This track's sources changed. Showing the latest."
        } catch {
            problem = "Automatic selection wasn't restored. Try again."
        }
        await load()
    }
}

struct SourceRow: View {
    let entry: TrackSources.Entry
    let chosen: Bool
    let choose: () -> Void

    var body: some View {
        Button(action: choose) {
            HStack(spacing: 12) {
                Image(systemName: chosen ? "largecircle.fill.circle" : "circle")
                    .foregroundStyle(chosen ? Theme.focus : Theme.quiet)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(entry.primary).font(Theme.bodyStrong).lineLimit(2)
                    Text(entry.secondary).font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(2)
                    HStack(spacing: 6) {
                        SourceChip(text: entry.state, color: entry.ready ? Theme.soft : Theme.warning)
                        if entry.pinned { SourceChip(text: "Pinned", color: Theme.focus) }
                        if entry.bestGuess { SourceChip(text: "Best guess", color: Theme.warning) }
                    }
                }
                Spacer(minLength: 0)
            }
            .padding(.vertical, 10)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }
}

struct SourceChip: View {
    let text: String
    let color: Color

    var body: some View {
        Text(text)
            .font(Theme.caption)
            .foregroundStyle(color)
            .padding(.horizontal, 8)
            .padding(.vertical, 2)
            .overlay(Capsule().strokeBorder(color.opacity(0.6)))
    }
}
