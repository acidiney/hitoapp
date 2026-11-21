import Testing
@testable import HitoKit

@Suite struct StallWatchTests {
    let start = ContinuousClock.now

    func at(_ seconds: Double) -> ContinuousClock.Instant {
        start + .milliseconds(Int(seconds * 1000))
    }

    var deadline: Double { Double(StallWatch.deadline.components.seconds) }

    @Test func aWaitingPlayerShowsBufferingAndTellsTheLockScreenItStopped() {
        var watch = StallWatch()
        watch.observe(.playing, at: at(0))
        #expect(!watch.buffering)
        #expect(watch.rate(at: 1.5) == 1.5)
        watch.observe(.waiting, at: at(1))
        #expect(watch.buffering)
        #expect(watch.rate(at: 1.5) == 0)
        watch.observe(.playing, at: at(2))
        #expect(!watch.buffering)
        #expect(watch.rate(at: 1) == 1)
    }

    @Test func aPausedPlayerIsNeitherBufferingNorPlaying() {
        var watch = StallWatch()
        watch.observe(.waiting, at: at(0))
        watch.observe(.stopped, at: at(1))
        #expect(!watch.buffering)
        #expect(watch.rate(at: 1) == 0)
        #expect(watch.due == nil)
        #expect(watch.step(at: at(100)) == nil)
    }

    @Test func aStallPastTheDeadlineRebuildsOnce() {
        var watch = StallWatch()
        watch.observe(.playing, at: at(0))
        watch.observe(.waiting, at: at(10))
        #expect(watch.due == at(10 + deadline))
        #expect(watch.step(at: at(10 + deadline - 1)) == nil)
        #expect(watch.step(at: at(10 + deadline)) == .rebuild)
        #expect(watch.step(at: at(10 + deadline + 1)) == nil)
        #expect(watch.buffering)
    }

    @Test func aRebuildThatDoesNotPlayMovesToTheNextSource() {
        var watch = StallWatch()
        watch.observe(.waiting, at: at(0))
        #expect(watch.step(at: at(deadline)) == .rebuild)
        watch.rebuilt(.answered, at: at(deadline + 1))
        #expect(watch.step(at: at(2 * deadline)) == nil)
        #expect(watch.step(at: at(2 * deadline + 1)) == .nextSource)
    }

    @Test func aRebuildThatPlaysOnlyBrieflyStillMovesOnAtTheNextStall() {
        var watch = StallWatch()
        watch.observe(.waiting, at: at(0))
        #expect(watch.step(at: at(deadline)) == .rebuild)
        watch.observe(.playing, at: at(deadline + 1))
        watch.observe(.waiting, at: at(deadline + 3))
        #expect(watch.step(at: at(2 * deadline + 3)) == .nextSource)
    }

    @Test func steadyPlayingAfterARebuildEarnsAnotherRebuild() {
        var watch = StallWatch()
        watch.observe(.waiting, at: at(0))
        #expect(watch.step(at: at(deadline)) == .rebuild)
        watch.observe(.playing, at: at(deadline + 1))
        watch.observe(.waiting, at: at(3 * deadline))
        #expect(watch.step(at: at(4 * deadline)) == .rebuild)
    }

    @Test func aFailedItemIsRebuiltBeforeHitoMovesOn() {
        var watch = StallWatch()
        watch.observe(.playing, at: at(0))
        #expect(watch.failed(at: at(1)) == .rebuild)
        #expect(watch.failed(at: at(2)) == .nextSource)
    }

    @Test func anUnreachableServerKeepsTheSourceAndIsAskedAgainEachDeadline() {
        var watch = StallWatch()
        watch.observe(.waiting, at: at(0))
        #expect(watch.step(at: at(deadline)) == .rebuild)
        watch.rebuilt(.unreachable, at: at(deadline + 20))
        #expect(watch.buffering)
        #expect(watch.step(at: at(2 * deadline + 19)) == nil)
        #expect(watch.step(at: at(2 * deadline + 20)) == .rebuild)
        watch.rebuilt(.answered, at: at(2 * deadline + 21))
        #expect(watch.step(at: at(3 * deadline + 21)) == .nextSource)
    }

    @Test func aServerThatStaysUnreachableEndsTheWait() {
        var watch = StallWatch()
        let wait = Double(StallWatch.serverWait.components.seconds)
        watch.observe(.waiting, at: at(0))
        #expect(watch.step(at: at(deadline)) == .rebuild)
        watch.rebuilt(.unreachable, at: at(deadline))
        var now = deadline
        var steps: [StallWatch.Step] = []
        while now < deadline + wait + 2 * deadline {
            now += deadline
            guard let step = watch.step(at: at(now)) else { continue }
            steps.append(step)
            if step == .giveUp { break }
            watch.rebuilt(.unreachable, at: at(now))
        }
        #expect(steps.last == .giveUp)
        #expect(steps.dropLast().allSatisfy { $0 == .rebuild })
        #expect(now >= deadline + wait)
    }

    @Test func playingAgainForgetsTheUnreachableServer() {
        var watch = StallWatch()
        let wait = Double(StallWatch.serverWait.components.seconds)
        watch.observe(.waiting, at: at(0))
        #expect(watch.step(at: at(deadline)) == .rebuild)
        watch.rebuilt(.unreachable, at: at(deadline))
        watch.observe(.playing, at: at(deadline + 2))
        watch.observe(.waiting, at: at(deadline + wait))
        #expect(watch.step(at: at(2 * deadline + wait)) == .rebuild)
    }
}
