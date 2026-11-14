import HitoKit
import SwiftUI
import UIKit

actor ArtworkCache {
    static let shared = ArtworkCache()

    private let memory = NSCache<NSURL, UIImage>()
    private var missing: Set<URL> = []
    private var pending: [URL: Task<UIImage?, Never>] = [:]

    func image(_ request: URLRequest) async -> UIImage? {
        guard let url = request.url else { return nil }
        if let image = memory.object(forKey: url as NSURL) { return image }
        if missing.contains(url) { return nil }
        if let running = pending[url] { return await running.value }
        let task = Task<UIImage?, Never> {
            guard let (data, response) = try? await URLSession.shared.data(for: request),
                  (response as? HTTPURLResponse)?.statusCode == 200 else { return nil }
            return UIImage(data: data)
        }
        pending[url] = task
        let image = await task.value
        pending[url] = nil
        if let image { memory.setObject(image, forKey: url as NSURL) } else { missing.insert(url) }
        return image
    }
}

struct Cover: View {
    @Environment(AppModel.self) private var model
    let path: [String]
    var size: CGFloat?
    var radius: CGFloat = 6
    @State private var image: UIImage?

    var body: some View {
        ZStack {
            Theme.tile
            if let image {
                Image(uiImage: image).resizable().scaledToFill()
            } else {
                Image(systemName: "music.note")
                    .font(.system(size: (size ?? 120) * 0.3))
                    .foregroundStyle(Theme.coverMark)
            }
        }
        .frame(width: size, height: size)
        .aspectRatio(1, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: radius))
        .accessibilityHidden(true)
        .task(id: path) {
            guard let api = model.api else { return }
            image = await ArtworkCache.shared.image(api.request(path))
        }
    }
}

struct TrackRow: View {
    let track: Track
    var showsCover = true
    var isCurrent = false

    var body: some View {
        HStack(spacing: 12) {
            if showsCover { Cover(path: track.artworkPath, size: 44) }
            VStack(alignment: .leading, spacing: 2) {
                Text(track.title)
                    .foregroundStyle(isCurrent ? Theme.focus : track.isPlayable ? Theme.text : Theme.soft)
                    .lineLimit(1)
                SourceLine(track: track)
            }
            Spacer(minLength: 8)
            Text(Duration.milliseconds(track.durationMs).clock)
                .font(Theme.small)
                .monospacedDigit()
                .foregroundStyle(Theme.quiet)
        }
        .frame(minHeight: 60)
        .contentShape(Rectangle())
    }
}

struct SourceLine: View {
    let track: Track

    var body: some View {
        HStack(spacing: 6) {
            if track.available {
                Circle().fill(Theme.soft).frame(width: 8, height: 8)
                Text("Ready · \(track.artist)")
            } else if track.playsFromServices {
                Circle().strokeBorder(Theme.focus, lineWidth: 1.5).frame(width: 8, height: 8)
                Text("From a service").foregroundStyle(Theme.focus)
                Text("· \(track.artist)")
            } else {
                Circle().strokeBorder(Theme.warning, lineWidth: 1.5).frame(width: 8, height: 8)
                Text("No source").foregroundStyle(Theme.warning)
                Text("· \(track.artist)")
            }
        }
        .font(Theme.small)
        .foregroundStyle(Theme.quiet)
        .lineLimit(1)
        .accessibilityElement(children: .combine)
    }
}

extension Duration {
    var clock: String {
        let seconds = Int(components.seconds)
        let hours = seconds / 3600
        let minutes = (seconds % 3600) / 60
        let rest = seconds % 60
        return hours > 0 ? String(format: "%d:%02d:%02d", hours, minutes, rest) : String(format: "%d:%02d", minutes, rest)
    }
}
