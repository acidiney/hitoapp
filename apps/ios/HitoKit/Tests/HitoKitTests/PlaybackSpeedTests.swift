import Testing
@testable import HitoKit

@Suite struct PlaybackSpeedTests {
    @Test func theChoicesMatchTheWebAndReadAsMultipliers() {
        #expect(PlaybackSpeed.choices == [0.5, 0.75, 1, 1.25, 1.5, 2])
        #expect(PlaybackSpeed.choices.map(PlaybackSpeed.label) == ["0.5×", "0.75×", "1×", "1.25×", "1.5×", "2×"])
    }
}
