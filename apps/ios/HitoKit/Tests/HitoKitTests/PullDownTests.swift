import Testing
@testable import HitoKit

@Suite struct PullDownTests {
    let height = 800.0

    func pulled(_ moves: (x: Double, y: Double)...) -> PullDown {
        var pull = PullDown()
        for move in moves { pull.drag(x: move.x, y: move.y) }
        return pull
    }

    @Test func aDownwardDragFollowsTheFinger() {
        let pull = pulled((2, 12), (4, 90))
        #expect(pull.axis == .vertical)
        #expect(pull.offset == 90)
    }

    @Test func nothingMovesBeforeTheDragPicksADirection() {
        let pull = pulled((3, 6))
        #expect(pull.axis == nil)
        #expect(pull.offset == 0)
        #expect(!pull.closes(velocity: 2000, height: height))
    }

    @Test func aShortSlowDragSpringsBack() {
        let pull = pulled((0, 20), (0, 120))
        #expect(!pull.closes(velocity: 200, height: height))
    }

    @Test func aLongDragCloses() {
        let pull = pulled((0, 20), (0, height / 4))
        #expect(pull.closes(velocity: 0, height: height))
    }

    @Test func aShortFastFlickCloses() {
        let pull = pulled((0, 20), (0, 60))
        #expect(pull.closes(velocity: 1200, height: height))
    }

    @Test func aTwitchIsNotAFlick() {
        let pull = pulled((0, 12))
        #expect(!pull.closes(velocity: 3000, height: height))
    }

    @Test func aLongDragPushedBackUpSpringsBack() {
        let pull = pulled((0, 20), (0, height / 2))
        #expect(!pull.closes(velocity: -600, height: height))
    }

    @Test func anUpwardDragDoesNotMoveThePlayer() {
        let pull = pulled((0, -20), (0, -200))
        #expect(pull.axis == .vertical)
        #expect(pull.offset == 0)
        #expect(!pull.closes(velocity: -1500, height: height))
    }

    @Test func aSidewaysDragIsLeftForTheArtworkSwipe() {
        let pull = pulled((20, 8), (60, 200))
        #expect(pull.axis == .horizontal)
        #expect(pull.offset == 0)
        #expect(!pull.closes(velocity: 2000, height: height))
    }

    @Test func aDiagonalDragCountsAsVerticalOnlyWhenMostlyDown() {
        #expect(pulled((11, 12)).axis == .vertical)
        #expect(pulled((12, 11)).axis == .horizontal)
    }
}
