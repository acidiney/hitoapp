import HitoKit
import SwiftUI

struct PrepareAudio: View {
    @Environment(AppModel.self) private var model
    let recording: String
    let source: String
    let prepared: () -> Void
    @State private var status: SourcePreparation.Status?
    @State private var requesting = false
    @State private var problem: String?
    @State private var watching: Task<Void, Never>?
    @State private var stalled = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if requesting {
                Text("Requesting audio preparation…").font(Theme.small).foregroundStyle(Theme.soft)
            } else if let status {
                Text(status.message).font(Theme.small).foregroundStyle(status.finished && !status.ready ? Theme.warning : Theme.soft)
                if let percent = status.percent {
                    ProgressView(value: Double(percent), total: 100)
                        .tint(Theme.focus)
                        .accessibilityLabel("Audio preparation progress")
                        .accessibilityValue("\(percent)%")
                }
                if let eta = status.eta { Text(eta).font(Theme.caption).foregroundStyle(Theme.quiet) }
                if let reason = status.reason { Text(reason).font(Theme.small).foregroundStyle(Theme.warning) }
            }
            if let problem { Text(problem).font(Theme.small).foregroundStyle(Theme.warning) }
            if stalled {
                TextAction(title: "Check again") {
                    stalled = false
                    problem = nil
                    watch()
                }
            }
            if !requesting, status == nil || (status?.finished == true && status?.ready == false) {
                Button(status == nil ? "Prepare audio" : "Retry preparation request") { Task { await start() } }
                    .buttonStyle(.plain)
                    .font(Theme.smallStrong)
                    .foregroundStyle(Theme.focus)
                    .frame(minHeight: 44)
            }
        }
        .task { await check() }
        .onDisappear { watching?.cancel() }
    }

    private func start() async {
        guard let api = model.api else { return }
        requesting = true
        defer { requesting = false }
        do {
            try await SourcePreparation(api: api).start(recording, source: source)
            problem = nil
            watch()
        } catch {
            problem = PluginTrouble.message(error, otherwise: "The music service didn't take the request. Try again.")
        }
    }

    private func check() async {
        guard let api = model.api else { return }
        status = try? await SourcePreparation(api: api).status(recording, source: source)
        if status?.finished == false { watch() }
    }

    private func watch() {
        watching?.cancel()
        watching = Task {
            guard let api = model.api else { return }
            var misses = 0
            while !Task.isCancelled {
                do {
                    let latest = try await SourcePreparation(api: api).status(recording, source: source)
                    misses = 0
                    problem = nil
                    if let latest {
                        status = latest
                        if latest.ready { prepared() }
                        if latest.finished { return }
                    }
                } catch is CancellationError {
                    return
                } catch {
                    misses += 1
                    model.handle(error)
                    if misses >= 3 {
                        problem = "Hito lost track of this preparation. It may still finish on the server."
                        stalled = true
                        return
                    }
                }
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }
}
