import AVFoundation
import HitoKit
import MediaPlayer
import Network
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
    private(set) var sourceId: String?
    private(set) var pickedSource = false
    private(set) var removed: (track: Track, undo: PlayQueue.Removed)?
    private(set) var liked: Set<String> = []
    private(set) var unliked: Set<String> = []
    private(set) var retrying: RetryPasses.Pass?
    private(set) var speed: Double = 1
    private var passes = RetryPasses()
    private let memory: QueueMemory
    private let listening: ListeningProgress
    private var heardId: String?
    private var checkpoint = ProgressCheckpoint()
    private var fallbackEnabled = true
    private var resumeAt: Double?
    private var chapters: [String: AudiobookAnchor] = [:]
    private var remembered: Double = 0
    private var playingAudio: AudioRequest?
    private var preflight: Task<Void, Never>?
    private var streamOffset: Double = 0
    private var streamQuality = StreamingQuality.original
    private var staysPaused = false

    private let playback: Playback
    private let likes: Likes
    private let api: HitoAPI
    private let engine = AVPlayer()
    private let log = Logger(subsystem: "app.hito", category: "player")
    private var loading: Task<Void, Never>?
    private var tried: Set<String> = []
    private static let leaseKey = "app.hito.lease"
    private static let leaseClientKey = "app.hito.lease-client"
    private var leaseId: String? {
        didSet { UserDefaults.standard.set(leaseId, forKey: Self.leaseKey) }
    }
    private var listed: Double = 0
    private var ticker: Any?
    private var observers: [NSObjectProtocol] = []
    private var itemStatus: NSKeyValueObservation?
    private var artworkTask: Task<Void, Never>?
    private var warm: (trackId: String, audio: AudioRequest)?
    private var warming: (task: Task<Void, Never>, id: UUID)?
    private var lookahead = Lookahead()
    private static let warmLead: Double = 30
    private(set) var stalls = StallWatch()
    private var deadline: Task<Void, Never>?
    private var engineStatus: NSKeyValueObservation?
    private let network = NWPathMonitor()

    var buffering: Bool { stalls.buffering }

    let account: String

    init(api: HitoAPI, account: String) {
        self.api = api
        self.account = account
        memory = QueueMemory(account: account)
        let previousClient = UserDefaults.standard.string(forKey: Self.leaseClientKey)
        let client = "ios-\(UUID().uuidString.lowercased())"
        UserDefaults.standard.set(client, forKey: Self.leaseClientKey)
        playback = Playback(api: api, client: client)
        if let previousClient {
            log.info("player.previous_run_leases_released")
            Task { [playback] in await playback.releaseLeft(by: previousClient) }
        }
        likes = Likes(api: api, device: Self.device)
        listening = ListeningProgress(api: api, device: Self.device)
        if let left = UserDefaults.standard.string(forKey: Self.leaseKey) {
            UserDefaults.standard.removeObject(forKey: Self.leaseKey)
            log.info("player.leftover_lease_released")
            Task { [playback] in await playback.release(left) }
        }
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
            MainActor.assumeIsolated { self?.stopped(item) }
        })
        observers.append(NotificationCenter.default.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] note in
            let shouldResume = (note.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt).map { AVAudioSession.InterruptionOptions(rawValue: $0).contains(.shouldResume) } ?? false
            let ended = (note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt) == AVAudioSession.InterruptionType.ended.rawValue
            MainActor.assumeIsolated { self?.interrupted(ended: ended, resume: shouldResume) }
        })
        observers.append(NotificationCenter.default.addObserver(forName: UIApplication.willResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.remember() }
        })
        watchPlayback()
        connectRemote()
        refreshFallback()
    }

    private func refreshFallback() {
        Task { [weak self, api] in
            guard let order = try? await SourceOrder(api: api).load() else { return }
            self?.fallbackEnabled = order.fallback
        }
    }

    func restore() async {
        guard queue.current == nil, let stored = memory.load() else { return }
        guard let found = try? await Library(api: api).lookup(Array(Set(stored.ids))),
              let restored = stored.restore(found), queue.current == nil else { return }
        queue.start(restored.tracks, at: restored.index, from: restored.context)
        position = restored.position
        duration = Double(restored.tracks[restored.index].durationMs) / 1000
        resumeAt = restored.position > 0 ? restored.position : nil
        log.info("player.queue_restored")
    }

    private func settle() {
        guard let id = heardId, let at = checkpoint.settle(position) else { return }
        report(id, at)
    }

    private func report(_ id: String, _ position: Double) {
        let chapter = chapters[id]
        Task { [listening, log, speed] in
            do {
                try await listening.save(id, position: position, speed: speed, chapter: chapter)
            } catch {
                log.info("player.progress_failed")
            }
        }
    }

    private func remember() {
        guard let current = queue.current else { return memory.forget() }
        var ids = queue.list.prefix(queue.position + 1).map(\.id)
        if ids.last != current.id { ids.append(current.id) }
        let index = ids.count - 1
        ids += queue.queued.map(\.id) + queue.upNext.map(\.id)
        memory.save(ids: ids, index: index, position: position, context: queue.context ?? "")
        remembered = position
    }

    var current: Track? { queue.current }

    func playAudiobook(_ tracks: [Track], anchors: [AudiobookAnchor], at index: Int, from context: String, startingAt seconds: Double = 0) {
        play(tracks, at: index, from: context, startingAt: seconds)
        chapters = Dictionary(anchors.map { ($0.recordingId, $0) }, uniquingKeysWith: { first, _ in first })
    }

    func play(_ tracks: [Track], at index: Int, from context: String, startingAt seconds: Double = 0) {
        chapters = [:]
        resumeAt = seconds > 0 ? seconds : nil
        queue.start(tracks, at: index, from: context)
        load(queue.current)
    }

    func toggle() {
        switch state {
        case .playing:
            state = .paused
            engine.pause()
            settle()
        case .paused where engine.currentItem == nil:
            load(queue.current)
        case .paused:
            activate()
            engine.play()
            state = .playing
        case .idle where queue.current != nil:
            load(queue.current)
        case .loading:
            loading?.cancel()
            retrying = nil
            state = .idle
        default:
            break
        }
        publish()
    }

    func next() {
        resumeAt = nil
        guard let track = moveOn(.chosen).next else { return }
        load(track)
    }

    func previous() {
        resumeAt = nil
        if position > 3 || queue.back() == nil {
            seek(to: 0)
            return
        }
        load(queue.current)
    }

    func seek(to seconds: Double) {
        position = seconds
        if streamQuality != .original, let playingAudio, playingAudio.leaseId == leaseId {
            resumeAt = seconds
            staysPaused = state == .paused
            engine.pause()
            stream(playingAudio, from: seconds)
            return
        }
        engine.seek(to: CMTime(seconds: seconds, preferredTimescale: 600))
        publish()
    }

    func jump(to track: Track) {
        resumeAt = nil
        guard let chosen = queue.jump(to: track.id) else { return }
        load(chosen)
    }

    func playNext(_ track: Track) {
        queue.playNext(track)
    }

    func clearUpcoming() {
        queue.clearUpcoming()
        removed = nil
    }

    func remove(upcoming track: Track) {
        guard let undo = queue.remove(upcoming: track.id) else { return }
        removed = (track, undo)
    }

    func undoRemove() {
        guard let removed else { return }
        queue.restore(removed.undo)
        self.removed = nil
    }

    func move(upcoming id: String, by offset: Int) {
        queue.move(upcoming: id, by: offset)
    }

    var shuffled: Bool { queue.shuffled }
    var repeatMode: PlayQueue.RepeatMode { queue.repeatMode }

    func toggleShuffle() {
        var generator = SystemRandomNumberGenerator()
        queue.setShuffle(!queue.shuffled, using: &generator)
    }

    func setSpeed(_ chosen: Double) {
        speed = chosen
        engine.defaultRate = Float(chosen)
        if state == .playing { engine.rate = Float(chosen) }
        log.info("player.speed_changed")
        publish()
    }

    func cycleRepeat() {
        queue.repeatMode = switch queue.repeatMode {
        case .off: .list
        case .list: .track
        case .track: .off
        }
    }

    func isLiked(_ track: Track) -> Bool {
        liked.contains(track.id) || (track.liked && !unliked.contains(track.id))
    }

    func toggleLike(_ track: Track) {
        let next = !isLiked(track)
        set(track.id, liked: next)
        Task { [likes, log] in
            do {
                try await likes.setLiked(track.id, next)
            } catch {
                log.info("player.like_failed")
                set(track.id, liked: !next)
                notice = next ? "Couldn't like \(track.title). Try again." : "Couldn't remove \(track.title) from Liked tracks. Try again."
            }
        }
    }

    private func set(_ id: String, liked value: Bool) {
        if value {
            liked.insert(id)
            unliked.remove(id)
        } else {
            liked.remove(id)
            unliked.insert(id)
        }
    }

    func sources() async throws -> [PlaybackSource] {
        guard let track = queue.current else { return [] }
        return try await playback.sources(track.id)
    }

    func play(source id: String) {
        guard let track = queue.current else { return }
        loading?.cancel()
        notice = nil
        state = .loading
        let resume = position
        loading = Task { [weak self, playback] in
            do {
                let audio = try await playback.prepare(track.id, source: id)
                guard let self, !Task.isCancelled else { return }
                self.tried = []
                self.attach(audio, picked: true)
                self.seek(to: resume)
            } catch is CancellationError {
            } catch {
                guard let self, !Task.isCancelled else { return }
                self.notice = "That source didn't play. Still playing from \(self.sourceLabel ?? "the last source")."
                self.state = self.engine.currentItem == nil ? .idle : .playing
            }
        }
    }

    func useAutomaticSource() {
        guard let track = queue.current, pickedSource else { return }
        loading?.cancel()
        notice = nil
        state = .loading
        let resume = position
        loading = Task { [weak self, playback] in
            do {
                let audio = try await playback.acquire(track.id, skipping: [])
                guard let self, !Task.isCancelled else { return }
                self.tried = []
                self.attach(audio)
                self.seek(to: resume)
            } catch is CancellationError {
            } catch {
                guard let self, !Task.isCancelled else { return }
                self.notice = "The automatic source didn't play. Still playing from \(self.sourceLabel ?? "the last source")."
                self.state = self.engine.currentItem == nil ? .idle : .playing
            }
        }
    }

    static var device: String {
        let key = "app.hito.device"
        if let saved = UserDefaults.standard.string(forKey: key) { return saved }
        let made = UUID().uuidString.lowercased()
        UserDefaults.standard.set(made, forKey: key)
        return made
    }

    func stop() {
        settle()
        loading?.cancel()
        dropWarm()
        engine.replaceCurrentItem(with: nil)
        releaseLease()
        state = .idle
        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    private func load(_ track: Track?) {
        settle()
        refreshFallback()
        defer { remember() }
        loading?.cancel()
        tried = []
        passes = RetryPasses()
        retrying = nil
        notice = nil
        guard let track else { return stop() }
        start(track)
    }

    private func releaseLease() {
        preflight?.cancel()
        preflight = nil
        playingAudio = nil
        streamOffset = 0
        streamQuality = .original
        staysPaused = false
        guard let leaseId else { return }
        self.leaseId = nil
        Task { [playback] in await playback.release(leaseId) }
    }

    private func start(_ track: Track) {
        lookahead = Lookahead()
        if let warm, warm.trackId == track.id, tried.isEmpty {
            self.warm = nil
            log.info("player.prewarm_used")
            startWarm(track, audio: warm.audio)
            return
        }
        dropWarm()
        releaseLease()
        state = .loading
        position = 0
        duration = Double(track.durationMs) / 1000
        listed = duration
        sourceLabel = nil
        sourceId = nil
        engine.pause()
        publish()
        loadArtwork(for: track)
        loading = Task { [weak self, playback, tried] in
            do {
                let audio = try await playback.acquire(track.id, skipping: tried)
                guard let self, !Task.isCancelled else { return }
                self.attach(audio)
            } catch is CancellationError {
            } catch HitoAPI.Problem.noAnswer {
                guard let self, !Task.isCancelled else { return }
                self.unreachable()
            } catch let Playback.Problem.notFound(reason, retryable) {
                guard let self, !Task.isCancelled else { return }
                self.log.info("player.not_found")
                if !retryable || !self.retry(track) { self.skip(track, because: "\(reason).") }
            } catch {
                guard let self, !Task.isCancelled else { return }
                self.log.info("player.no_source")
                let failed = !self.tried.isEmpty
                if !failed || !self.fallbackEnabled || !self.retry(track) {
                    self.skip(track, because: failed ? "\(track.title) didn't play from any source." : "\(track.title) has no source that plays right now.")
                }
            }
        }
    }

    private func startWarm(_ track: Track, audio: AudioRequest) {
        position = 0
        duration = Double(track.durationMs) / 1000
        listed = duration
        loadArtwork(for: track)
        attach(audio)
    }

    private func dropWarm() {
        warming?.task.cancel()
        warming = nil
        guard let warm else { return }
        self.warm = nil
        Task { [playback] in await playback.release(warm.audio.leaseId) }
    }

    private var lookaheadTarget: String? { lookahead.target(queue.upcoming.map(\.id)) }

    private func prewarm(remaining: Double) {
        let target = lookaheadTarget
        if let warm, warm.trackId != target { dropWarm() }
        guard state == .playing, remaining <= Self.warmLead, warming == nil, warm == nil,
              let target, target != queue.current?.id else { return }
        let id = UUID()
        let task = Task { [weak self, playback] in
            while true {
                let audio: AudioRequest
                do {
                    audio = try await playback.acquire(target)
                } catch {
                    guard let self, self.warming?.id == id, !Task.isCancelled else { return }
                    guard let wait = self.lookahead.missed(target, Lookahead.Miss(error)) else {
                        self.warming = nil
                        self.log.info("player.prewarm_missed")
                        return
                    }
                    self.log.info("player.prewarm_retry")
                    try? await Task.sleep(for: .seconds(wait))
                    continue
                }
                guard let self, self.warming?.id == id else {
                    Task { await playback.release(audio.leaseId) }
                    return
                }
                self.warming = nil
                guard !Task.isCancelled, self.lookaheadTarget == target else {
                    Task { await playback.release(audio.leaseId) }
                    return
                }
                self.lookahead.found(target)
                self.warm = (target, audio)
                self.log.info("player.prewarmed")
                return
            }
        }
        warming = (task, id)
    }

    private func moveOn(_ move: Lookahead.Move) -> (next: Track?, passed: [Track]) {
        let upcoming = queue.upcoming
        let passing = lookahead.passing(upcoming.map(\.id), on: move)
        guard passing < upcoming.count || passing == 0 else { return (nil, upcoming) }
        for _ in 0..<passing { queue.advance() }
        return (queue.advance(), Array(upcoming.prefix(passing)))
    }

    private func attach(_ audio: AudioRequest, picked: Bool = false) {
        if audio.leaseId != leaseId { releaseLease() }
        if audio.sourceId != sourceId || audio.recordingId != heardId { stalls = StallWatch() }
        retrying = nil
        if heardId != audio.recordingId {
            heardId = audio.recordingId
            checkpoint = ProgressCheckpoint()
        }
        leaseId = audio.leaseId
        tried.insert(audio.sourceId)
        sourceLabel = audio.sourceLabel
        sourceId = audio.sourceId
        pickedSource = picked
        playingAudio = audio
        stream(audio, from: resumeAt ?? 0)
    }

    private func stream(_ audio: AudioRequest, from seconds: Double) {
        let quality = StreamingQuality.preferred(account: account)
        let original = StreamChoice(url: audio.url, quality: .original, startOffset: 0, fallback: nil)
        preflight?.cancel()
        guard quality != .original else { return present(audio, choice: original) }
        state = .loading
        publish()
        preflight = Task { [weak self, playback] in
            let choice = (try? await playback.stream(audio, quality: quality, from: seconds)) ?? StreamChoice(url: audio.url, quality: .original, startOffset: 0, fallback: StreamChoice.originalOnly)
            guard let self, !Task.isCancelled, self.leaseId == audio.leaseId else { return }
            self.present(audio, choice: choice)
        }
    }

    private func present(_ audio: AudioRequest, choice: StreamChoice) {
        streamOffset = choice.startOffset
        streamQuality = choice.quality
        if let fallback = choice.fallback {
            notice = fallback
            log.info("player.quality_fallback")
        }
        let asset = AVURLAsset(url: choice.url, options: ["AVURLAssetHTTPHeaderFieldsKey": audio.headers])
        let item = AVPlayerItem(asset: asset)
        let identity = ObjectIdentifier(item)
        itemStatus = item.observe(\.status) { [weak self] item, _ in
            guard item.status == .failed else { return }
            Task { @MainActor in self?.stopped(identity) }
        }
        activate()
        engine.replaceCurrentItem(with: item)
        if let resumeAt {
            self.resumeAt = nil
            position = resumeAt
            if choice.quality == .original {
                engine.seek(to: CMTime(seconds: resumeAt, preferredTimescale: 600))
            }
        }
        if staysPaused {
            staysPaused = false
            state = .paused
        } else {
            engine.play()
            state = .playing
            log.info("player.started")
        }
        publish()
    }

    private var currentIdentity: ObjectIdentifier? { engine.currentItem.map(ObjectIdentifier.init) }

    private func failed(_ item: ObjectIdentifier?) {
        guard let item, item == currentIdentity, let track = queue.current else { return }
        engine.replaceCurrentItem(with: nil)
        if streamQuality != .original, let playingAudio, playingAudio.leaseId == leaseId {
            log.info("player.converted_failed")
            resumeAt = position
            present(playingAudio, choice: StreamChoice(url: playingAudio.url, quality: .original, startOffset: 0, fallback: StreamChoice.originalOnly))
            return
        }
        log.info("player.source_failed")
        let failedLabel = sourceLabel ?? "That source"
        guard SourceFallback.allows(tried: tried.count, enabled: fallbackEnabled) else {
            log.info("player.fallback_declined")
            return skip(track, because: fallbackEnabled ? "\(track.title) didn't play from any of its sources." : "\(failedLabel) didn't play \(track.title). Trying the next source is off in Settings.")
        }
        notice = "\(failedLabel) didn't play \(track.title). Trying the next source."
        start(track)
    }

    private func retry(_ track: Track) -> Bool {
        guard let pass = passes.next() else { return false }
        retrying = pass
        tried = []
        state = .loading
        log.info("player.retry")
        loading = Task { [weak self] in
            try? await Task.sleep(for: .seconds(pass.wait))
            guard let self, !Task.isCancelled else { return }
            self.start(track)
        }
        return true
    }

    private func watchPlayback() {
        engineStatus = engine.observe(\.timeControlStatus) { [weak self] _, _ in
            Task { @MainActor in self?.checkPlayback("engine") }
        }
        observers.append(NotificationCenter.default.addObserver(forName: AVPlayerItem.playbackStalledNotification, object: nil, queue: .main) { [weak self] note in
            let item = (note.object as AnyObject?).map(ObjectIdentifier.init)
            MainActor.assumeIsolated {
                guard let self, item == self.currentIdentity else { return }
                self.log.info("player.stalled")
                self.checkPlayback("stalled")
            }
        })
        observers.append(NotificationCenter.default.addObserver(forName: AVPlayerItem.newErrorLogEntryNotification, object: nil, queue: .main) { [weak self] note in
            let item = (note.object as AnyObject?).map(ObjectIdentifier.init)
            let entry = (note.object as? AVPlayerItem)?.errorLog()?.events.last
            let status = entry?.errorStatusCode ?? 0
            let domain = entry?.errorDomain ?? "unknown"
            MainActor.assumeIsolated {
                guard let self, item == self.currentIdentity else { return }
                self.log.info("player.stream_error domain=\(domain, privacy: .public) status=\(status, privacy: .public)")
                self.checkPlayback("stream_error")
            }
        })
        observers.append(NotificationCenter.default.addObserver(forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main) { [weak self] note in
            let reason = note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt ?? 0
            MainActor.assumeIsolated {
                self?.log.info("player.route_changed reason=\(reason, privacy: .public)")
                self?.checkPlayback("route")
            }
        })
        observers.append(NotificationCenter.default.addObserver(forName: UIApplication.willEnterForegroundNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.checkPlayback("foreground") }
        })
        network.pathUpdateHandler = { [weak self] path in
            let satisfied = path.status == .satisfied
            Task { @MainActor in
                self?.log.info("player.network_changed satisfied=\(satisfied, privacy: .public)")
                self?.checkPlayback("network")
            }
        }
        network.start(queue: .main)
    }

    private func checkPlayback(_ trigger: String) {
        let now = ContinuousClock.now
        let before = stalls.engine
        stalls.observe(truth(), at: now)
        if stalls.engine != before {
            if engine.currentItem?.status == .readyToPlay { tick(engine.currentTime().seconds) }
            log.info("player.engine_changed to=\(String(describing: self.stalls.engine), privacy: .public) trigger=\(trigger, privacy: .public) at_s=\(Int(self.position), privacy: .public)")
            publish()
        }
        if let step = stalls.step(at: now) { return recover(step) }
        scheduleDeadline()
    }

    private func truth() -> StallWatch.Engine {
        guard state == .playing else { return .stopped }
        switch engine.timeControlStatus {
        case .playing:
            return .playing
        case .paused where engine.currentItem?.status == .readyToPlay:
            log.info("player.paused_by_system")
            state = .paused
            settle()
            return .stopped
        default:
            return .waiting
        }
    }

    private func scheduleDeadline() {
        deadline?.cancel()
        guard let due = stalls.due else { return }
        deadline = Task { [weak self] in
            try? await Task.sleep(until: due, clock: .continuous)
            guard !Task.isCancelled else { return }
            self?.checkPlayback("deadline")
        }
    }

    private func stopped(_ item: ObjectIdentifier?) {
        guard let item, item == currentIdentity else { return }
        guard state == .playing else { return failed(item) }
        log.info("player.item_failed")
        let step = stalls.failed(at: .now)
        publish()
        scheduleDeadline()
        recover(step)
    }

    private func recover(_ step: StallWatch.Step) {
        switch step {
        case .rebuild:
            rebuild()
        case .nextSource:
            log.info("player.stall_next_source")
            resumeAt = position
            if let item = currentIdentity {
                failed(item)
            } else if let track = queue.current {
                start(track)
            }
        case .giveUp:
            log.info("player.stall_gave_up")
            unreachable()
        }
    }

    private func rebuild() {
        guard let audio = playingAudio, audio.leaseId == leaseId else { return }
        log.info("player.stall_rebuild at_s=\(Int(self.position), privacy: .public)")
        let at = position
        loading?.cancel()
        loading = Task { [weak self, playback] in
            let renewed: AudioRequest
            do {
                renewed = try await playback.renew(audio)
            } catch is CancellationError {
                return
            } catch HitoAPI.Problem.noAnswer {
                guard let self, !Task.isCancelled, self.leaseId == audio.leaseId else { return }
                self.log.info("player.stall_server_unreachable")
                self.stalls.rebuilt(.unreachable, at: .now)
                return self.checkPlayback("rebuild")
            } catch {
                guard let self, !Task.isCancelled, self.leaseId == audio.leaseId else { return }
                self.log.info("player.stall_rebuild_failed")
                return self.recover(.nextSource)
            }
            guard let self, !Task.isCancelled, self.leaseId == audio.leaseId, self.state == .playing else { return }
            if renewed.leaseId == audio.leaseId {
                self.log.info("player.stall_rebuilt lease=kept")
            } else {
                self.log.info("player.stall_rebuilt lease=renewed")
            }
            self.stalls.rebuilt(.answered, at: .now)
            self.resumeAt = at
            self.attach(renewed)
            self.checkPlayback("rebuilt")
        }
    }

    private func unreachable() {
        log.info("player.server_unreachable")
        if let resumeAt {
            position = resumeAt
        } else if position > 0 {
            resumeAt = position
        }
        retrying = nil
        engine.replaceCurrentItem(with: nil)
        state = .paused
        notice = "Hito can't reach your server. Press play to try again."
        checkPlayback("unreachable")
        publish()
    }

    private func skip(_ track: Track, because reason: String) {
        notice = reason
        resumeAt = nil
        passes = RetryPasses()
        retrying = nil
        engine.replaceCurrentItem(with: nil)
        if let following = queue.advance() {
            tried = []
            start(following)
        } else {
            releaseLease()
            state = .idle
            publish()
        }
    }

    private func ended(_ item: ObjectIdentifier?) {
        guard let item, item == currentIdentity else { return }
        let move = moveOn(.ended)
        if let following = move.next {
            load(following)
        } else {
            engine.pause()
            state = .paused
            seek(to: 0)
        }
        guard !move.passed.isEmpty else { return }
        log.info("player.boundary_passed count=\(move.passed.count, privacy: .public)")
        notice = Lookahead.notice(move.passed.map(\.title))
    }

    private func interrupted(ended: Bool, resume: Bool) {
        if !ended, state == .playing {
            state = .paused
            settle()
        } else if ended, resume, state == .paused {
            engine.play()
            state = .playing
        }
        publish()
    }

    private func tick(_ seconds: Double) {
        guard seconds.isFinite, state != .loading else { return }
        let seconds = seconds + streamOffset
        position = seconds
        let reported = engine.currentItem?.duration.seconds
        let stream: Double
        if let reported, reported.isFinite, reported > 0 {
            stream = reported + streamOffset
        } else if streamQuality != .original, listed > 0 {
            stream = listed
        } else {
            return
        }
        duration = PlaybackClock.length(stream: stream, listed: listed)
        prewarm(remaining: duration - seconds)
        if abs(seconds - remembered) >= 5 { remember() }
        if state == .playing, let id = heardId, let at = checkpoint.observe(seconds) { report(id, at) }
        if state == .playing, PlaybackClock.pastTheEnd(at: seconds, length: duration, stream: stream) {
            log.info("player.ended_at_real_length")
            ended(currentIdentity)
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
            self.artwork = Self.lockScreenArtwork(image)
            self.publish()
        }
    }

    private nonisolated static func lockScreenArtwork(_ image: UIImage) -> MPMediaItemArtwork {
        MPMediaItemArtwork(boundsSize: image.size) { @Sendable _ in image }
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
            MPNowPlayingInfoPropertyPlaybackRate: stalls.rate(at: speed),
            MPNowPlayingInfoPropertyDefaultPlaybackRate: speed,
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
