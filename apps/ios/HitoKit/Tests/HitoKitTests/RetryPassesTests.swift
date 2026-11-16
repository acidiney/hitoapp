import Testing
@testable import HitoKit

@Suite struct RetryPassesTests {
    @Test func anUnavailableTrackGetsThreePassesWithAShortWaitBetween() {
        var passes = RetryPasses()
        #expect(passes.attempt == 1)
        #expect(passes.next() == RetryPasses.Pass(attempt: 2, of: 3, wait: 0.5))
        #expect(passes.next() == RetryPasses.Pass(attempt: 3, of: 3, wait: 1))
        #expect(passes.next() == nil)
        #expect(passes.attempt == 3)
    }
}
