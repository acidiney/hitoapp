import Testing
@testable import HitoKit

@Suite struct PlayQueueTests {
    func track(_ id: String) -> Track {
        Track(id: id, title: id, artist: "Fixture Band", album: nil, releaseId: nil, durationMs: 1000, available: true, playsFromServices: false, liked: false)
    }

    var tracks: [Track] { ["a", "b", "c", "d"].map(track) }

    @Test func startingFromATrackPlaysItThenTheRestOfTheList() {
        var queue = PlayQueue()
        queue.start(tracks, at: 1, from: "Liked tracks")
        #expect(queue.current?.id == "b")
        #expect(queue.upNext.map(\.id) == ["c", "d"])
        #expect(queue.context == "Liked tracks")
    }

    @Test func tracksYouQueuePlayBeforeTheRestOfTheList() {
        var queue = PlayQueue()
        queue.start(tracks, at: 0, from: "Fixture Album")
        queue.playNext(track("x"))
        #expect(queue.queued.map(\.id) == ["x"])
        #expect(queue.advance()?.id == "x")
        #expect(queue.advance()?.id == "b")
        #expect(queue.queued.isEmpty)
    }

    @Test func goingBackReturnsToThePreviousTrackInTheList() {
        var queue = PlayQueue()
        queue.start(tracks, at: 2, from: "Fixture Album")
        #expect(queue.back()?.id == "b")
        #expect(queue.upNext.map(\.id) == ["c", "d"])
    }

    @Test func theEndOfTheListStopsWithoutWrapping() {
        var queue = PlayQueue()
        queue.start(tracks, at: 3, from: "Fixture Album")
        #expect(queue.advance() == nil)
        #expect(queue.current?.id == "d")
    }

    @Test func clearingUpcomingKeepsWhatIsPlaying() {
        var queue = PlayQueue()
        queue.start(tracks, at: 1, from: "Fixture Album")
        queue.playNext(track("x"))
        queue.clearUpcoming()
        #expect(queue.current?.id == "b")
        #expect(queue.upNext.isEmpty)
        #expect(queue.queued.isEmpty)
    }

    @Test func removingOneUpcomingTrackLeavesTheOthers() {
        var queue = PlayQueue()
        queue.start(tracks, at: 0, from: "Fixture Album")
        queue.playNext(track("x"))
        queue.remove(upcoming: "c")
        queue.remove(upcoming: "x")
        #expect(queue.queued.isEmpty)
        #expect(queue.upNext.map(\.id) == ["b", "d"])
    }

    @Test func tracksThatCannotPlayAreLeftOutOfTheList() {
        var queue = PlayQueue()
        var silent = track("b")
        silent = Track(id: silent.id, title: silent.title, artist: silent.artist, album: nil, releaseId: nil, durationMs: 1000, available: false, playsFromServices: false, liked: false)
        queue.start([track("a"), silent, track("c")], at: 0, from: "Fixture Album")
        #expect(queue.upNext.map(\.id) == ["c"])
    }
}

@Suite struct PlayQueueOrderTests {
    func track(_ id: String) -> Track {
        Track(id: id, title: id, artist: "Fixture Band", album: nil, releaseId: nil, durationMs: 1000, available: true, playsFromServices: false, liked: false)
    }

    var tracks: [Track] { ["a", "b", "c", "d", "e"].map(track) }

    @Test func shuffleKeepsTheCurrentTrackAndMixesOnlyWhatComesNext() {
        var queue = PlayQueue()
        queue.start(tracks, at: 1, from: "Fixture Album")
        var generator = FixedGenerator()
        queue.setShuffle(true, using: &generator)
        #expect(queue.current?.id == "b")
        #expect(Set(queue.upNext.map(\.id)) == ["c", "d", "e", "a"])
        #expect(queue.upNext.map(\.id) != ["c", "d", "e", "a"])
        queue.setShuffle(false, using: &generator)
        #expect(queue.current?.id == "b")
        #expect(queue.upNext.map(\.id) == ["c", "d", "e"])
    }

    @Test func repeatingTheListStartsAgainAfterTheLastTrack() {
        var queue = PlayQueue()
        queue.start(tracks, at: 4, from: "Fixture Album")
        queue.repeatMode = .list
        #expect(queue.advance()?.id == "a")
    }

    @Test func repeatingOneTrackPlaysItAgain() {
        var queue = PlayQueue()
        queue.start(tracks, at: 2, from: "Fixture Album")
        queue.repeatMode = .track
        #expect(queue.advance()?.id == "c")
        #expect(queue.upNext.map(\.id) == ["d", "e"])
    }

    @Test func upcomingTracksMoveUpAndDown() {
        var queue = PlayQueue()
        queue.start(tracks, at: 0, from: "Fixture Album")
        queue.move(upcoming: "d", by: -1)
        #expect(queue.upNext.map(\.id) == ["b", "d", "c", "e"])
        queue.move(upcoming: "d", by: 1)
        queue.move(upcoming: "e", by: 1)
        #expect(queue.upNext.map(\.id) == ["b", "c", "d", "e"])
        queue.playNext(track("x"))
        queue.playNext(track("y"))
        queue.move(upcoming: "y", by: -1)
        #expect(queue.queued.map(\.id) == ["y", "x"])
    }

    @Test func aRemovedTrackCanBePutBack() {
        var queue = PlayQueue()
        queue.start(tracks, at: 0, from: "Fixture Album")
        queue.playNext(track("x"))
        let first = queue.remove(upcoming: "c")
        let second = queue.remove(upcoming: "x")
        #expect(queue.upNext.map(\.id) == ["b", "d", "e"])
        queue.restore(try! #require(second))
        queue.restore(try! #require(first))
        #expect(queue.queued.map(\.id) == ["x"])
        #expect(queue.upNext.map(\.id) == ["b", "c", "d", "e"])
    }
}

struct FixedGenerator: RandomNumberGenerator {
    var state: UInt64 = 0x9E37_79B9_7F4A_7C15
    mutating func next() -> UInt64 {
        state = state &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
        return state
    }
}
