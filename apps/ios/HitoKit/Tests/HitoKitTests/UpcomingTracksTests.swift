import Foundation
import Testing
@testable import HitoKit

@Suite struct AnnouncementPaceTests {
    let start = ContinuousClock.now

    func at(_ seconds: Double) -> ContinuousClock.Instant {
        start + .milliseconds(Int(seconds * 1000))
    }

    var settle: Double { Double(AnnouncementPace.settle.components.seconds) }
    var spacing: Double { Double(AnnouncementPace.spacing.components.seconds) }

    func track(_ id: String) -> Track {
        Track(id: id, title: id, artist: "Fixture Band", album: nil, releaseId: nil, durationMs: 1000, available: true, playsFromServices: true, liked: false)
    }

    func queue(_ ids: [String], at index: Int = 0) -> PlayQueue {
        var queue = PlayQueue()
        queue.start(ids.map(track), at: index, from: "Fixture Album")
        return queue
    }

    @Test func anAnnouncementNamesThePlayingTrackAndTheNextTwoInPlayOrder() {
        var playing = queue(["a", "b", "c", "d"])
        #expect(Announcement(queue: playing, playing: true) == Announcement(current: "a", upcoming: ["b", "c"]))
        playing.playNext(track("x"))
        #expect(Announcement(queue: playing, playing: true) == Announcement(current: "a", upcoming: ["x", "b"]))
        #expect(Announcement(queue: playing, playing: false) == .stopped)
        #expect(Announcement.stopped == Announcement(current: nil, upcoming: []))
    }

    @Test func repeatAndTheEndOfTheListChangeWhatComesNext() {
        var playing = queue(["a", "b", "c"], at: 2)
        #expect(Announcement(queue: playing, playing: true).upcoming == [])
        playing.repeatMode = .list
        #expect(Announcement(queue: playing, playing: true).upcoming == ["a", "b"])
        playing.repeatMode = .track
        #expect(Announcement(queue: playing, playing: true) == Announcement(current: "c", upcoming: []))
    }

    @Test func aTrackStartIsAnnouncedOnceItSettles() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: ["b", "c"]), at: at(0))
        #expect(pace.due == at(settle))
        #expect(pace.take(at: at(settle - 0.1)) == nil)
        #expect(pace.take(at: at(settle)) == Announcement(current: "a", upcoming: ["b", "c"]))
        #expect(pace.due == nil)
    }

    @Test func nothingIsSentWhenNothingHasPlayed() {
        var pace = AnnouncementPace()
        pace.want(.stopped, at: at(0))
        #expect(pace.due == nil)
        #expect(pace.take(at: at(100)) == nil)
    }

    @Test func theSameAnnouncementIsNotSentAgain() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: ["b"]), at: at(0))
        _ = pace.take(at: at(settle))
        pace.finished(.delivered, at: at(settle))
        pace.want(Announcement(current: "a", upcoming: ["b"]), at: at(10))
        #expect(pace.due == nil)
        #expect(pace.take(at: at(20)) == nil)
    }

    @Test func aQueueEditThatChangesTheNextTwoIsAnnouncedAgain() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: ["b", "c"]), at: at(0))
        _ = pace.take(at: at(settle))
        pace.finished(.delivered, at: at(settle))
        pace.want(Announcement(current: "a", upcoming: ["c", "b"]), at: at(10))
        #expect(pace.take(at: at(10 + settle)) == Announcement(current: "a", upcoming: ["c", "b"]))
    }

    @Test func rapidSkipsAreCollapsedIntoTheLastOne() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: ["b", "c"]), at: at(0))
        pace.want(Announcement(current: "b", upcoming: ["c", "d"]), at: at(0.3))
        pace.want(Announcement(current: "c", upcoming: ["d", "e"]), at: at(0.6))
        #expect(pace.take(at: at(settle)) == nil)
        #expect(pace.due == at(0.6 + settle))
        #expect(pace.take(at: at(0.6 + settle)) == Announcement(current: "c", upcoming: ["d", "e"]))
    }

    @Test func repeatingAnUnchangedWishDoesNotPostponeIt() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: ["b"]), at: at(0))
        pace.want(Announcement(current: "a", upcoming: ["b"]), at: at(0.5))
        #expect(pace.due == at(settle))
    }

    @Test func stoppingAfterAnAnnouncementSendsNoCurrentTrack() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: ["b"]), at: at(0))
        _ = pace.take(at: at(settle))
        pace.finished(.delivered, at: at(settle))
        pace.want(.stopped, at: at(30))
        #expect(pace.take(at: at(30 + settle)) == .stopped)
    }

    @Test func stoppingBeforeAnythingWasSentSendsNothing() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: ["b"]), at: at(0))
        pace.want(.stopped, at: at(0.2))
        #expect(pace.due == nil)
    }

    @Test func announcementsAreSpacedSoOneDeviceStaysUnderTheServerLimit() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: []), at: at(0))
        _ = pace.take(at: at(settle))
        pace.finished(.delivered, at: at(settle))
        pace.want(Announcement(current: "b", upcoming: []), at: at(settle))
        #expect(pace.due == at(settle + spacing))
        #expect(spacing >= 2)
    }

    @Test func aChangeWhileOneIsInFlightWaitsForTheAnswer() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: []), at: at(0))
        #expect(pace.take(at: at(settle)) != nil)
        pace.want(Announcement(current: "b", upcoming: []), at: at(settle + 0.1))
        #expect(pace.due == nil)
        #expect(pace.take(at: at(10)) == nil)
        pace.finished(.delivered, at: at(10))
        #expect(pace.take(at: at(10)) == Announcement(current: "b", upcoming: []))
    }

    @Test func aLimitedAnnouncementWaitsForTheServersRetryAfter() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: ["b"]), at: at(0))
        _ = pace.take(at: at(settle))
        pace.finished(.limited(retryAfter: .seconds(17)), at: at(settle))
        #expect(pace.due == at(settle + 17))
        pace.want(Announcement(current: "b", upcoming: ["c"]), at: at(5))
        #expect(pace.due == at(settle + 17))
        #expect(pace.take(at: at(settle + 16)) == nil)
        #expect(pace.take(at: at(settle + 17)) == Announcement(current: "b", upcoming: ["c"]))
    }

    @Test func aLimitWithoutRetryAfterWaitsAMinute() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: []), at: at(0))
        _ = pace.take(at: at(settle))
        pace.finished(.limited(retryAfter: nil), at: at(settle))
        #expect(pace.due == at(settle + 60))
    }

    @Test func anUnansweredAnnouncementIsTriedAgainLater() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: []), at: at(0))
        _ = pace.take(at: at(settle))
        pace.finished(.failed, at: at(settle))
        let wait = Double(AnnouncementPace.failureWait.components.seconds)
        #expect(wait >= 10)
        #expect(pace.due == at(settle + wait))
        #expect(pace.take(at: at(settle + wait)) == Announcement(current: "a", upcoming: []))
    }

    @Test func aRejectedAnnouncementIsNotRepeated() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: []), at: at(0))
        _ = pace.take(at: at(settle))
        pace.finished(.rejected, at: at(settle))
        #expect(pace.due == nil)
        pace.want(Announcement(current: "b", upcoming: []), at: at(10))
        #expect(pace.take(at: at(10 + settle)) == Announcement(current: "b", upcoming: []))
    }

    @Test func aServerWithoutTheRouteIsNeverAskedAgain() {
        var pace = AnnouncementPace()
        pace.want(Announcement(current: "a", upcoming: []), at: at(0))
        _ = pace.take(at: at(settle))
        pace.finished(.unsupported, at: at(settle))
        pace.want(Announcement(current: "b", upcoming: []), at: at(10))
        #expect(pace.due == nil)
        #expect(pace.take(at: at(100)) == nil)
    }
}

@Suite struct UpcomingTracksTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let route = "POST /api/v1/playback/upcoming"

    func upcoming(_ fake: FakeServer) -> UpcomingTracks {
        UpcomingTracks(api: HitoAPI(address: address, token: "session-token-1", session: fake.session), client: "ios-fixture-device")
    }

    @Test func anAnnouncementSendsTheTagThePlayingTrackAndTheNextTwo() async throws {
        let fake = FakeServer(routes: [route: .text(204, "")])
        let outcome = await upcoming(fake).announce(Announcement(current: "r1", upcoming: ["r2", "r3", "r4"]))
        #expect(outcome == .delivered)
        let sent = try #require(fake.requests.first)
        #expect(sent.line == route)
        #expect(sent.authorization == "Bearer session-token-1")
        #expect(sent.header("Content-Type") == "application/json")
        let body = try #require(sent.object)
        #expect(body["client"] as? String == "ios-fixture-device")
        #expect(body["current"] as? String == "r1")
        #expect(body["recording_ids"] as? [String] == ["r2", "r3"])
        #expect(Set(body.keys) == ["client", "current", "recording_ids"])
    }

    @Test func aStopSendsANullCurrentTrack() async throws {
        let fake = FakeServer(routes: [route: .text(204, "")])
        _ = await upcoming(fake).announce(.stopped)
        let body = try #require(fake.requests.first?.object)
        #expect(body["current"] is NSNull)
        #expect(body["recording_ids"] as? [String] == [])
    }

    @Test func eachAnswerHasAnOutcome() async {
        func outcome(_ answer: FakeServer.Answer) async -> AnnouncementPace.Outcome {
            await upcoming(FakeServer(routes: [route: answer])).announce(Announcement(current: "r1", upcoming: []))
        }
        #expect(await outcome(.headed(429, #"{"error":"Too many requests"}"#, ["Retry-After": "17"])) == .limited(retryAfter: .seconds(17)))
        #expect(await outcome(.json(429, #"{"error":"Too many requests"}"#)) == .limited(retryAfter: nil))
        #expect(await outcome(.json(422, #"{"error":"Announce at most 2 upcoming recordings"}"#)) == .rejected)
        #expect(await outcome(.text(404, "not found")) == .unsupported)
        #expect(await outcome(.json(401, #"{"error":"signed out"}"#)) == .failed)
        #expect(await outcome(.json(500, #"{"error":"internal"}"#)) == .failed)
    }

    @Test func anUnreachableServerFailsWithoutThrowing() async {
        let fake = FakeServer(routes: [:], failures: ["http://192.168.1.20:8080": URLError(.cannotConnectToHost)])
        #expect(await upcoming(fake).announce(Announcement(current: "r1", upcoming: [])) == .failed)
    }

    @Test func eachAccountKeepsItsOwnDeviceTag() throws {
        let defaults = try #require(UserDefaults(suiteName: "upcoming-tag-\(UUID().uuidString)"))
        let first = UpcomingTracks.clientTag(defaults: defaults, account: "http://server|u1")
        #expect(UpcomingTracks.clientTag(defaults: defaults, account: "http://server|u1") == first)
        #expect(UpcomingTracks.clientTag(defaults: defaults, account: "http://server|u2") != first)
        #expect(first.hasPrefix("ios-"))
        #expect((1...64).contains(first.count))
        #expect(first.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || "-_.:".contains($0)) })
    }
}

@Suite struct KeepPlayedTracksTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let route = "/api/v1/account/automatic-copies"

    func copies(_ fake: FakeServer) -> AutomaticCopies {
        AutomaticCopies(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func theSettingShowsTheSpaceUsedAgainstTheBudget() async throws {
        let fake = FakeServer(routes: ["GET \(route)": .json(200, #"{"keep_played_tracks":true,"used_bytes":1288490189,"budget_bytes":5368709120}"#)])
        let setting = try #require(try await copies(fake).load())
        #expect(setting.on)
        #expect(setting.usedBytes == 1_288_490_189)
        #expect(setting.budgetBytes == 5_368_709_120)
        #expect(setting.usage == "1.2 GB of 5 GB used")
    }

    @Test func aServerWithoutTheSettingHidesIt() async throws {
        let fake = FakeServer(routes: ["GET \(route)": .json(404, #"{"error":"not found"}"#)])
        #expect(try await copies(fake).load() == nil)
    }

    @Test func otherFailuresAreNotHidden() async {
        let fake = FakeServer(routes: ["GET \(route)": .json(500, #"{"error":"internal"}"#)])
        await #expect(throws: HitoAPI.Problem.unexpected(500)) { try await copies(fake).load() }
    }

    @Test func turningItOffSavesAndReturnsTheNewSpace() async throws {
        let fake = FakeServer(routes: ["PUT \(route)": .json(200, #"{"keep_played_tracks":false,"used_bytes":0,"budget_bytes":5368709120}"#)])
        let setting = try await copies(fake).set(false)
        #expect(!setting.on)
        #expect(setting.usage == "0 MB of 5 GB used")
        let sent = try #require(fake.requests.first)
        #expect(sent.line == "PUT \(route)")
        #expect(sent.object?["keep_played_tracks"] as? Bool == false)
        #expect(sent.object?.count == 1)
    }

    @Test func smallAmountsReadInMegabytes() {
        #expect(KeepPlayedTracks(on: true, usedBytes: 52_428_800, budgetBytes: 1_073_741_824).usage == "50 MB of 1 GB used")
        #expect(KeepPlayedTracks(on: true, usedBytes: 0, budgetBytes: 0).usage == "0 MB of 0 MB used")
    }
}
