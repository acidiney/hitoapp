import Testing
@testable import HitoKit

@Suite struct LookaheadTests {
    let upcoming = ["b", "c", "d", "e"]

    @Test func aMissedTrackIsTriedOnceMoreAndPlaysWhenTheRetryFindsIt() {
        var ahead = Lookahead()
        #expect(ahead.target(upcoming) == "b")
        #expect(ahead.missed("b", .noSource(retryable: true)) == 0.5)
        #expect(ahead.target(upcoming) == "b")
        ahead.found("b")
        #expect(ahead.target(upcoming) == "b")
        #expect(ahead.passing(upcoming, on: .ended) == 0)
    }

    @Test func aTrackStillMissingAfterItsRetryIsPassedAtTheEndWithoutASearchAndTheOneAfterIsPrepared() {
        var ahead = Lookahead()
        #expect(ahead.missed("b", .noSource(retryable: true)) == 0.5)
        #expect(ahead.missed("b", .noSource(retryable: true)) == nil)
        #expect(ahead.target(upcoming) == "c")
        #expect(ahead.passing(upcoming, on: .ended) == 1)
    }

    @Test func aTrackNoServiceKnowsIsNotTriedAgain() {
        var ahead = Lookahead()
        #expect(ahead.missed("b", .noSource(retryable: false)) == nil)
        #expect(ahead.target(upcoming) == "c")
        #expect(ahead.passing(upcoming, on: .ended) == 1)
    }

    @Test func aTrackTheListenerChoosesStillGetsItsNormalSearch() {
        var ahead = Lookahead()
        _ = ahead.missed("b", .noSource(retryable: false))
        #expect(ahead.passing(upcoming, on: .chosen) == 0)
    }

    @Test func severalTracksInARowWithNoSourceAreAllPassedAtTheEnd() {
        var ahead = Lookahead()
        _ = ahead.missed("b", .noSource(retryable: true))
        _ = ahead.missed("b", .noSource(retryable: true))
        #expect(ahead.target(upcoming) == "c")
        _ = ahead.missed("c", .noSource(retryable: true))
        _ = ahead.missed("c", .noSource(retryable: true))
        #expect(ahead.target(upcoming) == "d")
        ahead.found("d")
        #expect(ahead.passing(upcoming, on: .ended) == 2)
    }

    @Test func whenNothingLeftHasASourceEveryTrackIsPassed() {
        var ahead = Lookahead()
        for id in upcoming { _ = ahead.missed(id, .noSource(retryable: false)) }
        #expect(ahead.target(upcoming) == nil)
        #expect(ahead.passing(upcoming, on: .ended) == upcoming.count)
    }

    @Test func aFailureThatIsNotAMissingSourceStopsLookingAheadAndLeavesTheTrackForTheEnd() {
        var ahead = Lookahead()
        #expect(ahead.missed("b", .inconclusive) == nil)
        #expect(ahead.target(upcoming) == nil)
        #expect(ahead.passing(upcoming, on: .ended) == 0)
    }

    @Test func aTrackMovedAheadOfAMissedOneIsPreparedFirst() {
        var ahead = Lookahead()
        _ = ahead.missed("b", .noSource(retryable: false))
        #expect(ahead.target(["x", "b", "c"]) == "x")
        #expect(ahead.passing(["x", "b", "c"], on: .ended) == 0)
    }

    @Test func errorsAreReadAsAMissingSourceOnlyWhenTheSearchFoundNothing() {
        #expect(Lookahead.Miss(Playback.Problem.notFound("No source found", retryable: true)) == .noSource(retryable: true))
        #expect(Lookahead.Miss(Playback.Problem.notFound("No service knows this track", retryable: false)) == .noSource(retryable: false))
        #expect(Lookahead.Miss(Playback.Problem.noReadySource) == .noSource(retryable: false))
        #expect(Lookahead.Miss(HitoAPI.Problem.noAnswer) == .inconclusive)
        #expect(Lookahead.Miss(HitoAPI.Problem.refused(500)) == .inconclusive)
    }

    @Test func theNoticeNamesWhatWasPassed() {
        #expect(Lookahead.notice(["Fixture One"]) == "Fixture One has no source that plays right now.")
        #expect(Lookahead.notice(["Fixture One", "Fixture Two"]) == "Fixture One and Fixture Two have no source that plays right now.")
        #expect(Lookahead.notice(["Fixture One", "Fixture Two", "Fixture Three"]) == "Fixture One and 2 more tracks have no source that plays right now.")
    }
}
