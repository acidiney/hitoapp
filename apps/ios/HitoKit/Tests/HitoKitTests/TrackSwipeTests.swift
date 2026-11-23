import Testing
@testable import HitoKit

@Suite struct TrackSwipeTests {
    let width = 340.0

    func swiped(_ x: Double, hasNext: Bool = true, hasPrevious: Bool = true) -> TrackSwipe {
        var swipe = TrackSwipe(hasNext: hasNext, hasPrevious: hasPrevious)
        swipe.drag(x: x)
        return swipe
    }

    @Test func theCoverFollowsTheFinger() {
        #expect(swiped(-80).offset == -80)
        #expect(swiped(80).offset == 80)
    }

    @Test func aLongSwipeLeftPlaysTheNextTrack() {
        #expect(swiped(-width * 0.3).turn(velocity: 0, width: width) == .next)
    }

    @Test func aLongSwipeRightPlaysThePreviousTrack() {
        #expect(swiped(width * 0.3).turn(velocity: 0, width: width) == .previous)
    }

    @Test func aShortSlowSwipeSpringsBack() {
        #expect(swiped(-60).turn(velocity: -200, width: width) == nil)
        #expect(swiped(60).turn(velocity: 200, width: width) == nil)
    }

    @Test func aShortFastFlickTurns() {
        #expect(swiped(-50).turn(velocity: -900, width: width) == .next)
        #expect(swiped(50).turn(velocity: 900, width: width) == .previous)
    }

    @Test func aTwitchIsNotAFlick() {
        #expect(swiped(-20).turn(velocity: -3000, width: width) == nil)
    }

    @Test func aLongSwipePushedBackSpringsBack() {
        #expect(swiped(-width / 2).turn(velocity: 600, width: width) == nil)
        #expect(swiped(width / 2).turn(velocity: -600, width: width) == nil)
    }

    @Test func theEndOfTheQueueHolds() {
        let swipe = swiped(-200, hasNext: false)
        #expect(swipe.offset == -200 * TrackSwipe.edgeResistance)
        #expect(swipe.turn(velocity: -2000, width: width) == nil)
    }

    @Test func withNoEarlierTrackTheCoverResistsAndPreviousStillRestarts() {
        let swipe = swiped(200, hasPrevious: false)
        #expect(swipe.offset == 200 * TrackSwipe.edgeResistance)
        #expect(swipe.turn(velocity: 0, width: width) == .previous)
    }

    @Test func anUntouchedSwipeDoesNothing() {
        let swipe = TrackSwipe(hasNext: true, hasPrevious: true)
        #expect(swipe.offset == 0)
        #expect(swipe.turn(velocity: 2000, width: width) == nil)
    }
}
