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
