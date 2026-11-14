import Testing
@testable import HitoKit

@Suite struct PasswordRuleTests {
    @Test func beforeTypingItSaysWhatIsNeeded() {
        #expect(PasswordRule.progress("") == "At least 12 characters. A long phrase is easier to remember.")
    }

    @Test func countsTowardTwelveThenSaysLongEnough() {
        #expect(PasswordRule.progress("correct h") == "9 of 12 characters")
        #expect(PasswordRule.progress("correct horse") == "Long enough")
        #expect(!PasswordRule.isLongEnough("correct h"))
        #expect(PasswordRule.isLongEnough("correct hors"))
    }

    @Test func countsBytesLikeTheServer() {
        #expect(PasswordRule.isLongEnough("ééééééé"))
        #expect(!PasswordRule.isLongEnough(String(repeating: "a", count: 1025)))
    }
}
