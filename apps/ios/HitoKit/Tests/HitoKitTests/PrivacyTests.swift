import Foundation
import Testing
@testable import HitoKit

@Suite struct PrivacyTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let lookups = #"{"revision":3,"enabled":true,"providers":[],"automatic_artist_lookup":true,"automatic_artwork":false,"automatic_service_search":true}"#

    func privacy(_ fake: FakeServer) -> Privacy {
        Privacy(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func privacySettingsLoadTogether() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/account/lock": .json(200, #"{"pin_enabled":true,"idle_minutes":15,"locked":false}"#),
            "GET /api/v1/history/preferences": .json(200, #"{"enabled":false}"#),
            "GET /api/v1/metadata/settings": .json(200, lookups),
        ])
        let settings = try await privacy(fake).load()
        #expect(settings == PrivacySettings(pinEnabled: true, idleMinutes: 15, history: false, artistLookup: true, artwork: false))
    }

    @Test func historyCanBeTurnedOffAndDeleted() async throws {
        let fake = FakeServer(routes: [
            "PUT /api/v1/history/preferences": .json(200, #"{"enabled":false}"#),
            "DELETE /api/v1/history": .json(200, #"{"removed":4}"#),
        ])
        #expect(try await privacy(fake).saveHistory(false) == false)
        try await privacy(fake).deleteHistory()
        #expect(fake.requests.first?.object?["enabled"] as? Bool == false)
        #expect(fake.requests.map(\.line) == ["PUT /api/v1/history/preferences", "DELETE /api/v1/history"])
        #expect(fake.requests.allSatisfy { $0.header("x-hito-request") == "1" })
    }

    @Test func eachAutomaticLookupIsSavedOnItsOwn() async throws {
        let fake = FakeServer(routes: ["PUT /api/v1/metadata/automation": .json(200, lookups)])
        let saved = try await privacy(fake).saveLookup(.artwork, false)
        #expect(fake.requests.first?.object?.keys.sorted() == ["automatic_artwork"])
        #expect(fake.requests.first?.object?["automatic_artwork"] as? Bool == false)
        #expect(saved.artwork == false && saved.artistLookup == true)
    }

    @Test func changingTheLockNeedsThePasswordAndKeepsThePinWhenNoneIsGiven() async throws {
        let fake = FakeServer(routes: ["PUT /api/v1/account/pin": .json(200, #"{"pin_enabled":true,"idle_minutes":60,"locked":false}"#)])
        let lock = try await privacy(fake).saveLock(pin: nil, idleMinutes: 60, password: "test-password")
        #expect(lock.idleMinutes == 60 && lock.pinEnabled)
        let body = try #require(fake.requests.first?.object)
        #expect(body["idle_minutes"] as? Int == 60)
        #expect(body["password"] as? String == "test-password")
        #expect(body["pin"] is NSNull)
    }
}
