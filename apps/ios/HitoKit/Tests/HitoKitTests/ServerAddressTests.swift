import Testing
@testable import HitoKit

@Suite struct ServerAddressTests {
    @Test func aTypedHttpsAddressIsTriedExactlyAsTyped() throws {
        let typed = try ServerAddressInput(parsing: "  https://music.example.com/library?x=1 ")
        #expect(typed.candidates.map(\.origin.absoluteString) == ["https://music.example.com"])
        #expect(typed.display == "music.example.com")
    }

    @Test func aTypedHttpAddressKeepsItsPortAndIsNotEncrypted() throws {
        let typed = try ServerAddressInput(parsing: "http://192.168.1.20:8080")
        #expect(typed.candidates.map(\.origin.absoluteString) == ["http://192.168.1.20:8080"])
        #expect(typed.candidates.first?.isEncrypted == false)
        #expect(typed.display == "192.168.1.20:8080")
    }

    @Test(arguments: ["hito.home:8080", "192.168.1.20:8080", "music.example.com", "[fd12::1]:8080"])
    func anAddressWithoutASchemeTriesHttpsThenHttp(input: String) throws {
        let typed = try ServerAddressInput(parsing: input)
        #expect(typed.candidates.map { $0.origin.scheme } == ["https", "http"])
        #expect(typed.candidates.allSatisfy { $0.origin.host() == typed.candidates[0].origin.host() })
    }

    @Test(arguments: ["", "   ", "my server", "ftp://music.example.com", "https://", "http://user:secret@music.example.com", "https://exa mple.com", "hito..home"])
    func refusesWhatIsNotAnAddress(input: String) {
        #expect(throws: ServerAddressInput.Problem.notAnAddress) { try ServerAddressInput(parsing: input) }
    }

    @Test func aSavedAddressDisplaysWithItsScheme() throws {
        let address = try #require(try ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates.first)
        #expect(address.origin.absoluteString == "http://192.168.1.20:8080")
        #expect(address.host == "192.168.1.20")
    }
}
