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
    var mark = "note"
    var round = false
    @State private var image: UIImage?

    var body: some View {
        ZStack {
            Theme.tile
            if let image {
                Image(uiImage: image).resizable().scaledToFill()
            } else {
                Icon(mark, size: min((size ?? 100) * 0.44, 44))
                    .foregroundStyle(Theme.coverMark)
            }
        }
        .frame(width: size, height: size)
        .frame(maxWidth: size == nil ? .infinity : nil)
        .aspectRatio(1, contentMode: .fit)
        .clipShape(round ? AnyShape(Circle()) : AnyShape(RoundedRectangle(cornerRadius: radius)))
        .accessibilityHidden(true)
        .task(id: path) {
            guard let api = model.api else { return }
            image = await ArtworkCache.shared.image(api.request(path))
        }
    }
}

struct AccountPicture: View {
    @Environment(AppModel.self) private var model
    let user: User
    var size: CGFloat = 44
    var fill = Theme.avatar
    var font = Theme.bodyBold
    @State private var image: UIImage?

    var body: some View {
        ZStack {
            Circle().fill(fill)
            if let image {
                Image(uiImage: image).resizable().scaledToFill()
            } else {
                Text(String((user.displayName ?? user.username).prefix(1)).uppercased()).font(font)
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .accessibilityHidden(true)
        .task(id: user.avatarVersion) {
            guard let version = user.avatarVersion, let api = model.api else {
                image = nil
                return
            }
            image = await ArtworkCache.shared.image(api.request(Profile.photoPath(of: user), query: [URLQueryItem(name: "v", value: version)]))
        }
    }
}

struct StatusDot: View {
    let track: Track
    var size: CGFloat = 8

    var body: some View {
        Group {
            if track.available {
                Circle().fill(Theme.soft)
            } else if track.playsFromServices {
                Circle().strokeBorder(Theme.focus, lineWidth: 1.5)
            } else {
                Circle().strokeBorder(Theme.warning, lineWidth: 1.5)
            }
        }
        .frame(width: size, height: size)
    }
}

extension Track {
    var stateLabel: String {
        available ? "Ready" : playsFromServices ? "From a service" : "No source"
    }

    var stateColor: Color {
        available ? Theme.quiet : playsFromServices ? Theme.focus : Theme.warning
    }

    var clock: String { Duration.milliseconds(durationMs).clock }
}

struct LibraryRow: View {
    let track: Track
    var isCurrent = false
    var showsTime = true

    var body: some View {
        HStack(spacing: 8) {
            Cover(path: track.artworkPath, size: 44)
            VStack(alignment: .leading, spacing: 0) {
                Text(track.title)
                    .foregroundStyle(isCurrent ? Theme.focus : Theme.text)
                    .lineLimit(1)
                HStack(spacing: 6) {
                    StatusDot(track: track)
                    Text(track.stateLabel).foregroundStyle(track.stateColor).layoutPriority(1)
                    Text("· \(track.artist)").lineLimit(1)
                }
                .font(Theme.small)
                .foregroundStyle(Theme.quiet)
            }
            Spacer(minLength: 8)
            if showsTime {
                Text(track.clock)
                    .font(Theme.small)
                    .monospacedDigit()
                    .foregroundStyle(Theme.quiet)
                    .padding(.trailing, 12)
            }
        }
        .frame(minHeight: 60)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}

struct SearchRow: View {
    let track: Track
    var isCurrent = false

    var body: some View {
        HStack(spacing: 12) {
            Cover(path: track.artworkPath, size: 44)
            VStack(alignment: .leading, spacing: 0) {
                Text(track.title)
                    .foregroundStyle(isCurrent ? Theme.focus : Theme.text)
                    .lineLimit(1)
                HStack(spacing: 6) {
                    StatusDot(track: track)
                    Text(track.stateLabel).foregroundStyle(track.available ? Theme.quiet : track.stateColor).layoutPriority(1)
                    Text("· \(track.clock)").lineLimit(1)
                }
                .font(Theme.small)
                .foregroundStyle(Theme.quiet)
            }
            Spacer(minLength: 8)
        }
        .frame(minHeight: 60)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}

struct AlbumTrackRow: View {
    let track: Track
    var isCurrent = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(track.title)
                .foregroundStyle(isCurrent ? Theme.focus : track.isPlayable ? Theme.text : Theme.soft)
                .lineLimit(1)
            HStack(spacing: 6) {
                StatusDot(track: track, size: 9)
                (Text("\(track.stateLabel) · ").foregroundStyle(track.available ? Theme.quiet : track.stateColor)
                    + Text(track.artist).foregroundStyle(Theme.quiet))
                    .lineLimit(1)
            }
            .font(Theme.small)
        }
        .frame(maxWidth: .infinity, minHeight: 60, alignment: .leading)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}

extension Duration {
    var long: String {
        let minutes = Int(components.seconds) / 60
        return minutes >= 60 ? "\(minutes / 60) hr \(minutes % 60) min" : "\(minutes) min"
    }

    var clock: String {
        let seconds = Int(components.seconds)
        let hours = seconds / 3600
        let minutes = (seconds % 3600) / 60
        let rest = seconds % 60
        return hours > 0 ? String(format: "%d:%02d:%02d", hours, minutes, rest) : String(format: "%d:%02d", minutes, rest)
    }
}
