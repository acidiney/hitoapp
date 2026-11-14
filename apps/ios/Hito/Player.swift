import AVFoundation
import HitoKit
import MediaPlayer
import OSLog
import UIKit

@MainActor
@Observable
final class Player {
    enum State: Equatable {
        case idle
        case loading
        case playing
        case paused
    }

    private(set) var queue = PlayQueue()
    private(set) var state = State.idle
    private(set) var position: Double = 0
    private(set) var duration: Double = 0
    private(set) var sourceLabel: String?
    private(set) var notice: String?

    private let playback: Playback
    private let api: HitoAPI
    private let engine = AVPlayer()
    private let log = Logger(subsystem: "app.hito", category: "player")
    private var loading: Task<Void, Never>?
    private var tried: Set<String> = []
    private var ticker: Any?
    private var observers: [NSObjectProtocol] = []
    private var itemStatus: NSKeyValueObservation?
    private var artworkTask: Task<Void, Never>?

    init(api: HitoAPI) {
        self.api = api
        playback = Playback(api: api)
        engine.automaticallyWaitsToMinimizeStalling = true
        ticker = engine.addPeriodicTimeObserver(forInterval: CMTime(seconds: 0.5, preferredTimescale: 600), queue: .main) { [weak self] time in
            MainActor.assumeIsolated { self?.tick(time.seconds) }
        }
        observers.append(NotificationCenter.default.addObserver(forName: AVPlayerItem.didPlayToEndTimeNotification, object: nil, queue: .main) { [weak self] note in
            let item = (note.object as AnyObject?).map(ObjectIdentifier.init)
            MainActor.assumeIsolated { self?.ended(item) }
        })
        observers.append(NotificationCenter.default.addObserver(forName: AVPlayerItem.failedToPlayToEndTimeNotification, object: nil, queue: .main) { [weak self] note in
            let item = (note.object as AnyObject?).map(ObjectIdentifier.init)
            MainActor.assumeIsolated { self?.failed(item) }
        })
        observers.append(NotificationCenter.default.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] note in
            let shouldResume = (note.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt).map { AVAudioSession.InterruptionOptions(rawValue: $0).contains(.shouldResume) } ?? false
            let ended = (note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt) == AVAudioSession.InterruptionType.ended.rawValue
            MainActor.assumeIsolated { self?.interrupted(ended: ended, resume: shouldResume) }
        })
        connectRemote()
    }

    var current: Track? { queue.current }

    func play(_ tracks: [Track], at index: Int, from context: String) {
        queue.start(tracks, at: index, from: context)
        load(queue.current)
    }

    func toggle() {
        switch state {
        case .playing:
            engine.pause()
            state = .paused
        case .paused:
            activate()
            engine.play()
            state = .playing
        case .idle where queue.current != nil:
            load(queue.current)
        default:
            break
        }
        publish()
    }

    func next() {
        guard let track = queue.advance() else { return }
        load(track)
    }

    func previous() {
        if position > 3 || queue.back() == nil {
            seek(to: 0)
            return
        }
        load(queue.current)
    }

    func seek(to seconds: Double) {
        position = seconds
        engine.seek(to: CMTime(seconds: seconds, preferredTimescale: 600))
        publish()
    }

    func playNext(_ track: Track) {
        queue.playNext(track)
    }

    func clearUpcoming() {
        queue.clearUpcoming()
    }

    func remove(upcoming id: String) {
        queue.remove(upcoming: id)
    }

    func stop() {
        loading?.cancel()
        engine.replaceCurrentItem(with: nil)
        state = .idle
        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    private func load(_ track: Track?) {
        loading?.cancel()
        tried = []
        notice = nil
        guard let track else { return stop() }
        start(track)
    }

    private func start(_ track: Track) {
        state = .loading
        position = 0
        duration = Double(track.durationMs) / 1000
        sourceLabel = nil
        engine.pause()
        publish()
        loadArtwork(for: track)
        loading = Task { [weak self, playback, tried] in
            do {
                let audio = try await playback.prepare(track.id, skipping: tried)
                guard let self, !Task.isCancelled else { return }
                self.attach(audio)
            } catch is CancellationError {
            } catch {
                guard let self, !Task.isCancelled else { return }
                self.log.info("player.no_source")
                self.skip(track, because: self.tried.isEmpty ? "\(track.title) has no source that plays right now." : "\(track.title) didn't play from any source.")
            }
        }
    }

    private func attach(_ audio: AudioRequest) {
        tried.insert(audio.sourceId)
        sourceLabel = audio.sourceLabel
        let asset = AVURLAsset(url: audio.url, options: ["AVURLAssetHTTPHeaderFieldsKey": audio.headers])
        let item = AVPlayerItem(asset: asset)
        let identity = ObjectIdentifier(item)
        itemStatus = item.observe(\.status) { [weak self] item, _ in
            guard item.status == .failed else { return }
            Task { @MainActor in self?.failed(identity) }
        }
        activate()
        engine.replaceCurrentItem(with: item)
        engine.play()
        state = .playing
        log.info("player.started")
        publish()
    }

    private var currentIdentity: ObjectIdentifier? { engine.currentItem.map(ObjectIdentifier.init) }

    private func failed(_ item: ObjectIdentifier?) {
        guard let item, item == currentIdentity, let track = queue.current else { return }
        log.info("player.source_failed")
        let failedLabel = sourceLabel ?? "That source"
        notice = "\(failedLabel) didn't play \(track.title). Trying the next source."
        engine.replaceCurrentItem(with: nil)
        start(track)
    }

    private func skip(_ track: Track, because reason: String) {
        notice = reason
        engine.replaceCurrentItem(with: nil)
        if let following = queue.advance() {
            tried = []
            start(following)
        } else {
            state = .idle
            publish()
        }
    }

    private func ended(_ item: ObjectIdentifier?) {
        guard let item, item == currentIdentity else { return }
        if let following = queue.advance() {
            load(following)
        } else {
            state = .paused
            seek(to: 0)
        }
    }

    private func interrupted(ended: Bool, resume: Bool) {
        if !ended, state == .playing {
            state = .paused
        } else if ended, resume, state == .paused {
            engine.play()
            state = .playing
        }
        publish()
    }

    private func tick(_ seconds: Double) {
        guard seconds.isFinite else { return }
        position = seconds
        if let length = engine.currentItem?.duration.seconds, length.isFinite, length > 0 {
            duration = length
        }
    }

    private func activate() {
        let session = AVAudioSession.sharedInstance()
        try? session.setCategory(.playback, mode: .default, policy: .longFormAudio)
        try? session.setActive(true)
    }

    private var artwork: MPMediaItemArtwork?

    private func loadArtwork(for track: Track) {
        artwork = nil
        artworkTask?.cancel()
        artworkTask = Task { [weak self, api] in
            guard let image = await ArtworkCache.shared.image(api.request(track.artworkPath)) else { return }
            guard let self, !Task.isCancelled, self.queue.current?.id == track.id else { return }
            self.artwork = MPMediaItemArtwork(boundsSize: image.size) { _ in image }
            self.publish()
        }
    }

    private func publish() {
        guard let track = queue.current else {
            MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
            return
        }
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: track.title,
            MPMediaItemPropertyArtist: track.artist,
            MPMediaItemPropertyPlaybackDuration: duration,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: position,
            MPNowPlayingInfoPropertyPlaybackRate: state == .playing ? 1.0 : 0.0,
        ]
        if let album = track.album { info[MPMediaItemPropertyAlbumTitle] = album }
        if let artwork { info[MPMediaItemPropertyArtwork] = artwork }
        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
    }

    private func connectRemote() {
        let center = MPRemoteCommandCenter.shared()
        center.playCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { if self?.state == .paused { self?.toggle() } }
            return .success
        }
        center.pauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { if self?.state == .playing { self?.toggle() } }
            return .success
        }
        center.togglePlayPauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.toggle() }
            return .success
        }
        center.nextTrackCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.next() }
            return .success
        }
        center.previousTrackCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.previous() }
            return .success
        }
        center.changePlaybackPositionCommand.addTarget { [weak self] event in
            guard let event = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            MainActor.assumeIsolated { self?.seek(to: event.positionTime) }
            return .success
        }
    }
}
