import HitoKit
import SwiftUI

struct NowPlayingView: View {
    @Environment(\.dismiss) private var dismiss
    let player: Player
    let openQueue: () -> Void
    @State private var scrubbing: Double?

    var body: some View {
        VStack(spacing: 20) {
            HStack {
                Button { dismiss() } label: {
                    Image(systemName: "chevron.down").font(.system(size: 20, weight: .semibold))
                        .frame(width: 44, height: 44)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close now playing")
                Spacer()
            }
            .padding(.horizontal, -10)
            if let track = player.current {
                Cover(path: track.artworkPath, radius: 16)
                    .frame(maxWidth: .infinity)
                    .shadow(color: .black.opacity(0.45), radius: 28, y: 24)
                VStack(alignment: .leading, spacing: 2) {
                    if let context = player.queue.context {
                        Text("Playing from \(context)").font(Theme.label).foregroundStyle(Theme.soft)
                    }
                    Text(track.title).font(Theme.playing).lineLimit(2)
                    Text(track.artist).foregroundStyle(Theme.soft).lineLimit(1)
                    HStack(spacing: 8) {
                        Circle().fill(player.notice == nil ? Theme.text : Theme.warning).frame(width: 9, height: 9)
                        Text(status).foregroundStyle(player.notice == nil ? Theme.soft : Theme.warning)
                    }
                    .font(Theme.small)
                    .padding(.top, 4)
                    .accessibilityElement(children: .combine)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                VStack(spacing: 8) {
                    Slider(
                        value: Binding(get: { scrubbing ?? player.position }, set: { scrubbing = $0 }),
                        in: 0...max(player.duration, 1)
                    ) { editing in
                        if !editing, let value = scrubbing {
                            player.seek(to: value)
                            scrubbing = nil
                        }
                    }
                    .tint(Theme.text)
                    .accessibilityLabel("Seek")
                    .accessibilityValue("\(Duration.seconds(scrubbing ?? player.position).clock) of \(Duration.seconds(player.duration).clock)")
                    HStack {
                        Text(Duration.seconds(scrubbing ?? player.position).clock)
                        Spacer()
                        Text("-" + Duration.seconds(max(player.duration - (scrubbing ?? player.position), 0)).clock)
                    }
                    .font(Theme.caption)
                    .monospacedDigit()
                    .foregroundStyle(Theme.soft)
                    .accessibilityHidden(true)
                }
                HStack(spacing: 28) {
                    Button(action: player.previous) {
                        Image(systemName: "backward.end.fill").font(.system(size: 24)).frame(width: 56, height: 56)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Previous track")
                    PlayButton(player: player, size: 72, light: true)
                    Button(action: player.next) {
                        Image(systemName: "forward.end.fill").font(.system(size: 24)).frame(width: 56, height: 56)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Next track")
                    .disabled(!player.queue.hasNext)
                }
                Spacer(minLength: 0)
                Button(action: openQueue) {
                    Text("Up next")
                        .font(Theme.bodyStrong)
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .background(Capsule().fill(Theme.text.opacity(0.1)))
                }
                .buttonStyle(.plain)
            } else {
                Spacer()
                Text("Nothing is playing.").foregroundStyle(Theme.soft)
                Spacer()
            }
        }
        .padding(.horizontal, 24)
        .padding(.top, 12)
        .padding(.bottom, 20)
        .background(Theme.stage.ignoresSafeArea())
        .foregroundStyle(Theme.text)
    }

    private var status: String {
        if let notice = player.notice { return notice }
        if player.state == .loading { return "Finding a source…" }
        return player.sourceLabel.map { "Ready · from \($0)" } ?? "Ready"
    }
}

struct QueueSheet: View {
    @Environment(\.dismiss) private var dismiss
    let player: Player

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 4) {
                    Text("Queue").font(Theme.playing).accessibilityAddTraits(.isHeader)
                    Spacer()
                    if player.queue.hasNext {
                        TextAction(title: "Clear upcoming") { player.clearUpcoming() }
                    }
                    Button { dismiss() } label: {
                        Image(systemName: "xmark").font(.system(size: 16, weight: .semibold)).frame(width: 44, height: 44)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Close queue")
                }
                if let current = player.current {
                    Text("Now playing · stays when you clear").font(Theme.label).foregroundStyle(Theme.quiet)
                    HStack(spacing: 12) {
                        Cover(path: current.artworkPath, size: 44)
                        VStack(alignment: .leading, spacing: 0) {
                            Text(current.title).font(Theme.bodyStrong).foregroundStyle(Theme.focus).lineLimit(1)
                            Text(current.artist).font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(1)
                        }
                        Spacer(minLength: 0)
                    }
                    .padding(8)
                    .background(RoundedRectangle(cornerRadius: 10).fill(Theme.raised))
                }
                if !player.queue.queued.isEmpty {
                    Text("Queued by you · plays first").font(Theme.label).foregroundStyle(Theme.quiet).padding(.top, 4)
                    ForEach(player.queue.queued) { QueueRow(track: $0, player: player) }
                }
                if !player.queue.upNext.isEmpty {
                    Text("Next from \(player.queue.context ?? "this list")").font(Theme.label).foregroundStyle(Theme.quiet).padding(.top, 4)
                    ForEach(player.queue.upNext) { QueueRow(track: $0, player: player) }
                }
                if !player.queue.hasNext {
                    Text("Nothing is queued after this track.").font(Theme.small).foregroundStyle(Theme.soft).padding(.top, 8)
                }
            }
            .padding(EdgeInsets(top: 16, leading: 16, bottom: 24, trailing: 8))
        }
        .background(Theme.field.ignoresSafeArea())
        .foregroundStyle(Theme.text)
        .presentationDetents([.large])
        .presentationDragIndicator(.visible)
    }
}

struct QueueRow: View {
    let track: Track
    let player: Player

    var body: some View {
        HStack(spacing: 12) {
            Cover(path: track.artworkPath, size: 44)
            VStack(alignment: .leading, spacing: 0) {
                Text(track.title).lineLimit(1)
                Text(track.artist).font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(1)
            }
            Spacer(minLength: 0)
            Menu {
                Button("Remove from queue", role: .destructive) { player.remove(upcoming: track.id) }
            } label: {
                Image(systemName: "ellipsis").font(.system(size: 18, weight: .bold))
                    .foregroundStyle(Theme.quiet)
                    .frame(width: 44, height: 44)
            }
            .accessibilityLabel("Actions for \(track.title)")
        }
        .frame(minHeight: 60)
    }
}
