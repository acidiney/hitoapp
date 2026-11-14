import Testing
@testable import HitoKit

@Suite struct PlaybackClockTests {
    @Test func aStreamLengthCloseToTheLibraryOneIsTrusted() {
        #expect(PlaybackClock.length(stream: 175, listed: 172) == 175)
    }

    @Test func aStreamThatClaimsTwiceItsLengthIsHalved() {
        #expect(PlaybackClock.length(stream: 518, listed: 259) == 259)
        #expect(PlaybackClock.length(stream: 369, listed: 172) == 184.5)
    }

    @Test func aStreamLengthFarFromTheLibraryOneIsNotTrusted() {
        #expect(PlaybackClock.length(stream: 900, listed: 172) == 172)
    }

    @Test func withoutALibraryLengthTheStreamIsUsed() {
        #expect(PlaybackClock.length(stream: 200, listed: 0) == 200)
    }

    @Test func playingPastTheRealLengthOfALongerStreamIsTheEnd() {
        #expect(PlaybackClock.pastTheEnd(at: 185, length: 184.5, stream: 369))
        #expect(!PlaybackClock.pastTheEnd(at: 120, length: 184.5, stream: 369))
        #expect(!PlaybackClock.pastTheEnd(at: 185, length: 185, stream: 185))
    }
}
