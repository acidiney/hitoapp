import HitoKit
import SwiftUI

struct NowPlayingView: View {
    enum Panel: Equatable {
        case artwork, lyrics, source
    }

    @Environment(\.dismiss) private var dismiss
    let player: Player
    let openQueue: () -> Void
    @State private var panel = Panel.artwork

    var body: some View {
        Group {
            if let track = player.current {
                if panel == .artwork {
                    artwork(track)
                } else {
                    panels(track)
                }
            } else {
                VStack {
                    Spacer()
                    Text("Nothing is playing.").foregroundStyle(Theme.soft)
                    Spacer()
                }
                .frame(maxWidth: .infinity)
            }
        }
        .background(Theme.stage.ignoresSafeArea())
        .foregroundStyle(Theme.text)
    }

    private func artwork(_ track: Track) -> some View {
        VStack(spacing: 20) {
            HStack {
                IconButton(icon: "chevron-down", label: "Close now playing", glyph: 22) { dismiss() }
                Spacer()
                Menu {
                    Button(player.isLiked(track) ? "Remove from Liked tracks" : "Like") { player.toggleLike(track) }
                    Button("Up next", action: openQueue)
                } label: {
                    Icon("more").foregroundStyle(Theme.text).frame(width: 44, height: 44).contentShape(Circle())
                }
                .accessibilityLabel("More actions")
            }
            .padding(.horizontal, -10)
            Cover(path: track.artworkPath, radius: 16)
                .shadow(color: .black.opacity(0.45), radius: 28, y: 24)
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    if let context = player.queue.context {
                        Text("Playing from \(context)").font(Theme.label).foregroundStyle(Theme.soft).lineLimit(1)
                    }
                    Text(track.title).font(Theme.playing).lineLimit(2)
                    Text(track.artist).foregroundStyle(Theme.soft).lineLimit(1)
                    HStack(spacing: 8) {
                        Circle().fill(player.notice == nil ? Theme.text : Theme.warning).frame(width: 9, height: 9)
                        Text(status).foregroundStyle(player.notice == nil ? Theme.soft : Theme.warning).lineLimit(2)
                    }
                    .font(Theme.small)
                    .padding(.top, 4)
                    .accessibilityElement(children: .combine)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                LikeButton(player: player, track: track)
            }
            VStack(spacing: 8) {
                Seek(player: player)
                HStack {
                    Text(Duration.seconds(player.position).clock)
                    Spacer()
                    Text("-" + Duration.seconds(max(player.duration - player.position, 0)).clock)
                }
                .font(Theme.caption)
                .monospacedDigit()
                .foregroundStyle(Theme.soft)
                .accessibilityHidden(true)
            }
            HStack {
                IconButton(icon: "shuffle", label: "Shuffle", size: 48, glyph: 22, color: player.shuffled ? Theme.focus : Theme.soft, action: player.toggleShuffle)
                    .accessibilityAddTraits(player.shuffled ? .isSelected : [])
                Spacer()
                IconButton(icon: "previous", label: "Previous track", size: 56, glyph: 24, action: player.previous)
                Spacer()
                PlayButton(player: player, size: 72, light: true, glyph: 28)
                Spacer()
                IconButton(icon: "next", label: "Next track", size: 56, glyph: 24, action: player.next)
                    .disabled(!player.queue.hasNext)
                Spacer()
                RepeatButton(player: player)
            }
            Spacer(minLength: 0)
            PanelPicker(selection: .artwork) { pick($0) }
        }
        .padding(.horizontal, 24)
        .padding(.bottom, 4)
    }

    private func panels(_ track: Track) -> some View {
        VStack(spacing: 14) {
            HStack(spacing: 12) {
                IconButton(icon: "chevron-down", label: "Close panel, back to artwork", glyph: 22) { panel = .artwork }
                Cover(path: track.artworkPath, size: 48)
                VStack(alignment: .leading, spacing: 0) {
                    Text(track.title).font(Theme.bodyBold).lineLimit(1)
                    Text(player.subtitle).font(Theme.small).foregroundStyle(Theme.soft).lineLimit(1)
                }
                Spacer(minLength: 0)
                PlayButton(player: player, size: 48, light: true)
            }
            HStack(spacing: 10) {
                Text(Duration.seconds(player.position).clock)
                Seek(player: player)
                Text(Duration.seconds(player.duration).clock)
            }
            .font(Theme.caption)
            .monospacedDigit()
            .foregroundStyle(Theme.soft)
            PanelPicker(selection: panel) { pick($0) }
            switch panel {
            case .lyrics: LyricsPanel(player: player, track: track)
            case .source: SourcePanel(player: player, track: track)
            case .artwork: EmptyView()
            }
        }
        .padding(.horizontal, 16)
        .padding(.bottom, 28)
    }

    private func pick(_ choice: PanelPicker.Choice) {
        switch choice {
        case .lyrics: panel = .lyrics
        case .next: openQueue()
        case .source: panel = .source
        }
    }

    private var status: String {
        if let notice = player.notice { return notice }
        if player.state == .loading { return "Finding a source…" }
        return player.sourceLabel.map { "Ready · from \($0)" } ?? "Ready"
    }
}

struct PanelPicker: View {
    enum Choice: CaseIterable {
        case lyrics, next, source

        var label: String {
            switch self {
            case .lyrics: "Lyrics"
            case .next: "Up next"
            case .source: "Source"
            }
        }
    }

    let selection: NowPlayingView.Panel
    let choose: (Choice) -> Void

    var body: some View {
        HStack(spacing: 4) {
            ForEach(Choice.allCases, id: \.self) { choice in
                let selected = (choice == .lyrics && selection == .lyrics) || (choice == .source && selection == .source)
                Button { choose(choice) } label: {
                    Text(choice.label)
                        .font(Theme.bodyMedium)
                        .foregroundStyle(selected ? Theme.stage : Theme.text)
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .background(Capsule().fill(selected ? Theme.text : .clear))
                        .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(selected ? .isSelected : [])
            }
        }
        .padding(4)
        .background(Capsule().fill(Theme.text.opacity(0.1)))
    }
}

struct LikeButton: View {
    let player: Player
    let track: Track

    var body: some View {
        let liked = player.isLiked(track)
        IconButton(icon: liked ? "heart-filled" : "heart", label: "Like", glyph: 22, color: liked ? Theme.liked : Theme.soft) {
            player.toggleLike(track)
        }
        .accessibilityAddTraits(liked ? .isSelected : [])
    }
}

struct RepeatButton: View {
    let player: Player

    var body: some View {
        Button(action: player.cycleRepeat) {
            ZStack(alignment: .topTrailing) {
                Icon("repeat", size: 22)
                if player.repeatMode == .track {
                    Text("1").font(Theme.tabCurrent).offset(x: 6, y: -6)
                }
            }
            .foregroundStyle(player.repeatMode == .off ? Theme.soft : Theme.focus)
            .frame(width: 48, height: 48)
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Repeat")
        .accessibilityValue(player.repeatMode == .off ? "Off" : player.repeatMode == .list ? "All" : "This track")
    }
}

struct Seek: View {
    let player: Player
    @State private var dragging: Double?

    var body: some View {
        GeometryReader { geometry in
            let total = max(player.duration, 1)
            let value = dragging ?? player.position
            let fraction = CGFloat(min(max(value / total, 0), 1))
            ZStack(alignment: .leading) {
                Capsule().fill(Theme.rest).frame(height: 4)
                Capsule().fill(Theme.text).frame(width: geometry.size.width * fraction, height: 4)
                Circle().fill(Theme.text).frame(width: 12, height: 12)
                    .offset(x: geometry.size.width * fraction - 6)
            }
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { drag in
                    dragging = Double(min(max(drag.location.x / geometry.size.width, 0), 1)) * total
                }
                .onEnded { _ in
                    if let dragging { player.seek(to: dragging) }
                    dragging = nil
                })
        }
        .frame(height: 44)
        .padding(.vertical, -10)
        .accessibilityElement()
        .accessibilityLabel("Seek")
        .accessibilityValue("\(Duration.seconds(player.position).clock) of \(Duration.seconds(player.duration).clock)")
        .accessibilityAdjustableAction { direction in
            let step = direction == .increment ? 5.0 : -5.0
            player.seek(to: min(max(player.position + step, 0), player.duration))
        }
    }
}

struct LyricsPanel: View {
    @Environment(AppModel.self) private var model
    let player: Player
    let track: Track
    @State private var result: LyricsResult?
    @State private var failed = false
    @State private var following = true

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            switch result {
            case let .some(.lines(lines, provider)):
                let current = lines.lastIndex { $0.start <= player.position } ?? 0
                ScrollViewReader { reader in
                    ZStack(alignment: .bottom) {
                        ScrollView {
                            VStack(alignment: .leading, spacing: 14) {
                                ForEach(Array(lines.enumerated()), id: \.offset) { index, line in
                                    Button {
                                        player.seek(to: line.start)
                                        following = true
                                    } label: {
                                        Text(line.text)
                                            .font(index == current ? Theme.lyricCurrent : Theme.lyric)
                                            .foregroundStyle(index == current ? Theme.text : index < current ? Theme.quiet : Theme.soft)
                                            .multilineTextAlignment(.leading)
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                    }
                                    .buttonStyle(.plain)
                                    .id(index)
                                }
                            }
                            .padding(.vertical, 40)
                        }
                        .scrollIndicators(.hidden)
                        .simultaneousGesture(DragGesture().onChanged { _ in following = false })
                        if !following {
                            Button {
                                following = true
                                withAnimation { reader.scrollTo(current, anchor: .center) }
                            } label: {
                                HStack(spacing: 8) {
                                    Icon("arrow-up", size: 14)
                                    Text("Back to current line").font(Theme.smallMedium)
                                }
                                .foregroundStyle(Theme.stage)
                                .padding(.horizontal, 18)
                                .frame(minHeight: 44)
                                .background(Capsule().fill(Theme.text))
                                .shadow(color: .black.opacity(0.4), radius: 12, y: 8)
                            }
                            .buttonStyle(.plain)
                            .padding(.bottom, 8)
                        }
                    }
                    .onChange(of: current) { _, line in
                        guard following else { return }
                        withAnimation { reader.scrollTo(line, anchor: .center) }
                    }
                }
                Text("from \(provider)").font(Theme.caption).foregroundStyle(Theme.soft)
            case .some(.none):
                Spacer()
                VStack(alignment: .leading, spacing: 6) {
                    Text("No lyrics for this track").font(Theme.bodyStrong)
                    Text("Lyrics come from a plugin that offers them. None of your plugins has these.")
                        .font(Theme.small)
                        .foregroundStyle(Theme.soft)
                }
                Spacer()
            case .none:
                Spacer()
                if failed {
                    HStack(spacing: 8) {
                        Text("Lyrics didn't load.").foregroundStyle(Theme.soft)
                        TextAction(title: "Try again") { Task { await load() } }
                    }
                    .font(Theme.small)
                } else {
                    ProgressView().tint(Theme.soft).frame(maxWidth: .infinity)
                }
                Spacer()
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        .task(id: track.id) { await load() }
    }

    private func load() async {
        guard let api = model.api else { return }
        result = nil
        failed = false
        do {
            result = try await Lyrics(api: api).load(track.id)
        } catch {
            failed = true
            model.handle(error)
        }
    }
}

struct SourcePanel: View {
    @Environment(AppModel.self) private var model
    let player: Player
    let track: Track
    @State private var sources: [PlaybackSource]?
    @State private var failed = false
    @State private var copy: SavedCopy?
    @State private var copyBusy = false
    @State private var copyProblem: String?
    @State private var copyRequest = UUID()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 10) {
                if let current = sources?.first(where: { $0.id == player.sourceId }) {
                    Text("Playing from").font(Theme.label).foregroundStyle(Theme.soft).padding(.top, 4)
                    HStack(spacing: 12) {
                        Circle().fill(Theme.text).frame(width: 10, height: 10)
                        VStack(alignment: .leading, spacing: 0) {
                            Text(current.label).font(Theme.bodyBold)
                            Text(player.pickedSource ? "Ready · you picked it" : "Ready · picked automatically")
                                .font(Theme.small)
                                .foregroundStyle(Theme.soft)
                        }
                        Spacer(minLength: 0)
                        Text("Current").font(Theme.smallStrong).foregroundStyle(Theme.soft)
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 12)
                    .background(RoundedRectangle(cornerRadius: 16).fill(Theme.text.opacity(0.1)))
                }
                let others = (sources ?? []).filter { $0.id != player.sourceId }
                if !others.isEmpty {
                    Text("Other sources for this track").font(Theme.label).foregroundStyle(Theme.soft).padding(.top, 6)
                    ForEach(others) { source in
                        HStack(spacing: 12) {
                            if source.ready {
                                Circle().fill(Theme.text).frame(width: 10, height: 10)
                            } else {
                                Circle().strokeBorder(Theme.warning, lineWidth: 1.5).frame(width: 10, height: 10)
                            }
                            VStack(alignment: .leading, spacing: 0) {
                                Text(source.label).font(Theme.bodyBold)
                                Text(note(source))
                                    .font(Theme.small)
                                    .foregroundStyle(source.ready ? Theme.soft : Theme.warning)
                            }
                            Spacer(minLength: 0)
                            if source.ready {
                                Button { player.play(source: source.id) } label: {
                                    Text("Use")
                                        .font(Theme.smallMedium)
                                        .foregroundStyle(Theme.stage)
                                        .padding(.horizontal, 14)
                                        .frame(minHeight: 44)
                                        .background(Capsule().fill(Theme.text))
                                }
                                .buttonStyle(.plain)
                                .accessibilityLabel("Use \(source.label)")
                            }
                        }
                        .padding(.horizontal, 14)
                        .padding(.vertical, 12)
                        .background(RoundedRectangle(cornerRadius: 16).fill(Theme.text.opacity(0.06)))
                    }
                }
                if let sources, sources.contains(where: { $0.provider != "local" }) || copy != nil {
                    savedCopy
                }
                if sources == nil {
                    if failed {
                        HStack(spacing: 8) {
                            Text("Sources didn't load.").foregroundStyle(Theme.soft)
                            TextAction(title: "Try again") { Task { await load() } }
                        }
                        .font(Theme.small)
                    } else {
                        ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
                    }
                }
            }
        }
        .scrollIndicators(.hidden)
        .task(id: "\(track.id)-\(player.sourceId ?? "")") { await load() }
        .task(id: track.id) { await followCopy() }
    }

    private var savedCopy: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 12) {
                if copy?.state == .ready {
                    Circle().fill(Theme.text).frame(width: 10, height: 10)
                } else {
                    Circle().strokeBorder(copy?.state == .failed ? Theme.warning : Theme.soft, lineWidth: 1.5).frame(width: 10, height: 10)
                }
                VStack(alignment: .leading, spacing: 0) {
                    Text("Saved copy").font(Theme.bodyBold)
                    Text(copy?.summary ?? "Save a copy on your server to listen when the music service can't play it.")
                        .font(Theme.small)
                        .foregroundStyle(copy?.state == .failed ? Theme.warning : Theme.soft)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
                Button { Task { await (copy == nil ? saveCopy() : removeCopy()) } } label: {
                    Text(copy == nil ? "Save" : copy?.transferring == true ? "Cancel" : "Remove")
                        .font(Theme.smallMedium)
                        .foregroundStyle(copy == nil ? Theme.stage : Theme.text)
                        .padding(.horizontal, 14)
                        .frame(minHeight: 44)
                        .background(Capsule().fill(copy == nil ? Theme.text : .clear))
                        .overlay(Capsule().strokeBorder(copy == nil ? .clear : Theme.text.opacity(0.4)))
                        .opacity(copyBusy ? 0.5 : 1)
                }
                .buttonStyle(.plain)
                .disabled(copyBusy)
                .accessibilityLabel(copy == nil ? "Save a copy on your server" : copy?.transferring == true ? "Cancel saving the copy" : "Remove the saved copy")
            }
            if let copy, copy.transferring {
                if let progress = copy.progress {
                    ProgressView(value: progress).tint(Theme.text)
                } else {
                    ProgressView().progressViewStyle(.linear).tint(Theme.text)
                }
            }
            if let copyProblem {
                Text(copyProblem).font(Theme.small).foregroundStyle(Theme.warning)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(RoundedRectangle(cornerRadius: 16).fill(Theme.text.opacity(0.06)))
    }

    private func followCopy() async {
        guard let api = model.api else { return }
        let copies = SavedCopies(api: api)
        while !Task.isCancelled {
            copy = try? await copies.status(track.id)
            guard copy?.transferring == true else { return }
            try? await Task.sleep(for: .seconds(2))
        }
    }

    private func saveCopy() async {
        guard let api = model.api else { return }
        copyBusy = true
        copyProblem = nil
        defer { copyBusy = false }
        do {
            _ = try await SavedCopies(api: api).save(track.id, request: copyRequest)
            copyRequest = UUID()
            await followCopy()
        } catch let refused as SavedCopies.Refused {
            copyRequest = UUID()
            copyProblem = refused.reason
        } catch {
            copyProblem = "The copy didn't start. Try again."
            model.handle(error)
        }
    }

    private func removeCopy() async {
        guard let api = model.api, let intent = copy?.intentId else { return }
        copyBusy = true
        copyProblem = nil
        defer { copyBusy = false }
        do {
            try await SavedCopies(api: api).remove(intent)
            copy = nil
        } catch {
            copyProblem = "The copy is still there. Try again."
            model.handle(error)
        }
    }

    private func note(_ source: PlaybackSource) -> String {
        if source.missingFile { return "No file in your music folders" }
        if !source.ready { return "Not ready yet" }
        return source.bestGuess ? "Ready · best guess" : "Ready"
    }

    private func load() async {
        failed = false
        do {
            sources = try await player.sources()
        } catch {
            failed = true
            model.handle(error)
        }
    }
}

struct QueueSheet: View {
    @Environment(\.dismiss) private var dismiss
    let player: Player

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Capsule().fill(Theme.line).frame(width: 40, height: 4).frame(maxWidth: .infinity).accessibilityHidden(true)
            HStack(spacing: 4) {
                Text("Queue").font(Theme.playing).accessibilityAddTraits(.isHeader)
                Spacer()
                if player.queue.hasNext {
                    TextAction(title: "Clear upcoming") { player.clearUpcoming() }
                }
                IconButton(icon: "close", label: "Close queue") { dismiss() }
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 10) {
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
                        .padding(.trailing, 8)
                    }
                    if !player.queue.queued.isEmpty {
                        Text("Queued by you · plays first").font(Theme.label).foregroundStyle(Theme.quiet).padding(.top, 4)
                        VStack(spacing: 0) {
                            ForEach(player.queue.queued) { QueueRow(track: $0, player: player, strong: true, among: player.queue.queued) }
                        }
                    }
                    if !player.queue.upNext.isEmpty {
                        Text("Next from \(player.queue.context ?? "this list")").font(Theme.label).foregroundStyle(Theme.quiet).padding(.top, 4)
                        VStack(spacing: 0) {
                            ForEach(player.queue.upNext) { QueueRow(track: $0, player: player, strong: $0.id == player.queue.upNext.first?.id, among: player.queue.upNext) }
                        }
                    }
                    if !player.queue.hasNext {
                        Text("Nothing is queued after this track.").font(Theme.small).foregroundStyle(Theme.soft).padding(.top, 8)
                    }
                }
            }
            .scrollIndicators(.hidden)
            if let removed = player.removed {
                HStack {
                    Text("Removed \(removed.track.title) from the queue").lineLimit(2)
                    Spacer(minLength: 8)
                    TextAction(title: "Undo") { player.undoRemove() }
                }
                .padding(.leading, 14)
                .padding(.trailing, 10)
                .padding(.vertical, 8)
                .background(RoundedRectangle(cornerRadius: 10).fill(Theme.menu))
                .padding(.trailing, 8)
            }
            Text("Tracks without a source are skipped.").font(Theme.small).foregroundStyle(Theme.quiet)
        }
        .padding(EdgeInsets(top: 10, leading: 16, bottom: 24, trailing: 8))
        .background(Theme.field.ignoresSafeArea())
        .foregroundStyle(Theme.text)
        .presentationDetents([.large])
        .presentationCornerRadius(24)
    }
}

struct QueueRow: View {
    let track: Track
    let player: Player
    let strong: Bool
    let among: [Track]

    var body: some View {
        HStack(spacing: 12) {
            Cover(path: track.artworkPath, size: 44)
            VStack(alignment: .leading, spacing: 0) {
                Text(track.title).font(strong ? Theme.bodyStrong : Theme.body).lineLimit(1)
                Text(track.artist).font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(1)
            }
            Spacer(minLength: 0)
            Menu {
                Button("Move up") { player.move(upcoming: track.id, by: -1) }
                    .disabled(among.first?.id == track.id)
                Button("Move down") { player.move(upcoming: track.id, by: 1) }
                    .disabled(among.last?.id == track.id)
                Button("Remove from queue", role: .destructive) { player.remove(upcoming: track) }
            } label: {
                Icon("more-vertical").foregroundStyle(Theme.quiet).frame(width: 44, height: 44).contentShape(Circle())
            }
            .accessibilityLabel("Actions for \(track.title): move up, move down, remove")
        }
        .frame(minHeight: 60)
    }
}
