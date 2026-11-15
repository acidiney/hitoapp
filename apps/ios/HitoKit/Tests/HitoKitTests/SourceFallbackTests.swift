import Testing
@testable import HitoKit

@Suite struct SourceFallbackTests {
    @Test func theFirstSourceIsAlwaysTried() {
        #expect(SourceFallback.allows(tried: 0, enabled: false))
        #expect(SourceFallback.allows(tried: 0, enabled: true))
    }

    @Test func withFallbackOffAFailedSourceIsTheLastOne() {
        #expect(!SourceFallback.allows(tried: 1, enabled: false))
    }

    @Test func withFallbackOnUpToSixSourcesAreTriedLikeTheWeb() {
        #expect(SourceFallback.allows(tried: 5, enabled: true))
        #expect(!SourceFallback.allows(tried: 6, enabled: true))
    }
}
