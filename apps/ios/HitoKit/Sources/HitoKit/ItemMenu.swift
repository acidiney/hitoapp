public enum ItemAction: CaseIterable, Hashable, Sendable {
    case play, shuffle, playNext, startRadio, open
    case details, findAudio, fixSource
    case like, unlike
    case chooseArtist, edit
    case moveUp, moveDown
    case select
    case retryArtwork
    case removeFromQueue, removeFromPlaylist, delete

    public var title: String {
        switch self {
        case .play: "Play"
        case .shuffle: "Shuffle"
        case .playNext: "Play next"
        case .startRadio: "Start radio"
        case .open: "Open"
        case .details: "Track details"
        case .findAudio: "Find audio"
        case .fixSource: "Fix source"
        case .like: "Like"
        case .unlike: "Remove from Liked tracks"
        case .chooseArtist: "Choose the artist"
        case .edit: "Edit playlist"
        case .moveUp: "Move up"
        case .moveDown: "Move down"
        case .select: "Select"
        case .retryArtwork: "Retry artwork"
        case .removeFromQueue: "Remove from queue"
        case .removeFromPlaylist: "Remove from playlist"
        case .delete: "Delete playlist"
        }
    }

    public var isDestructive: Bool {
        switch self {
        case .removeFromQueue, .removeFromPlaylist, .delete: true
        default: false
        }
    }
}

public enum ItemMenu {
    public static func track(_ track: Track, liked: Bool?, artworkMissing: Bool) -> [ItemAction] {
        var actions: [ItemAction] = [.playNext, .details]
        if !track.id.hasPrefix(Track.unlinked) {
            actions.append(track.isPlayable ? .fixSource : .findAudio)
        }
        if let liked {
            actions.append(liked ? .unlike : .like)
        }
        if artworkMissing { actions.append(.retryArtwork) }
        return actions
    }

    public static func album(artworkMissing: Bool) -> [ItemAction] {
        [.play, .playNext, .chooseArtist] + (artworkMissing ? [.retryArtwork] : [])
    }

    public static func playlist(_ playlist: Playlist, ready: Int?, artworkMissing: Bool) -> [ItemAction] {
        var actions: [ItemAction] = ready == 0 ? [] : [.play, .playNext]
        if playlist.kind == .local { actions.append(.edit) }
        if artworkMissing { actions.append(.retryArtwork) }
        actions.append(.delete)
        return actions
    }

    public static let collection: [ItemAction] = [.play, .shuffle]

    public static let radio: [ItemAction] = [.startRadio]

    public static let featured: [ItemAction] = [.play, .open]

    public static let queued: [ItemAction] = [.moveUp, .moveDown, .removeFromQueue]
}
