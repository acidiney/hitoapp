import Foundation
import Testing
@testable import HitoKit

@Suite struct MetadataProvidersTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func providers(_ fake: FakeServer) -> MetadataProviders {
        MetadataProviders(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    let settings = #"{"revision":4,"enabled":true,"providers":[{"kind":"http_plugin","installation_id":"i1"},{"kind":"builtin_musicbrainz"},{"kind":"http_plugin","installation_id":"gone"}],"automatic_artist_lookup":true,"automatic_artwork":true,"automatic_service_search":false}"#

    let fixture = PluginConnection(id: "i1", name: "Owned fake", capabilities: ["catalog", "search"], health: .ok, revision: 2, checkedAt: nil)
    let other = PluginConnection(id: "i2", name: "Other", capabilities: ["catalog"], health: .unreachable, revision: 1, checkedAt: nil)
    let searchOnly = PluginConnection(id: "i3", name: "Search only", capabilities: ["search"], health: .ok, revision: 1, checkedAt: nil)

    @Test func providersLoadInPriorityOrderWithTheirLabels() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/metadata/settings": .json(200, settings)])
        let loaded = try await providers(fake).load()
        #expect(loaded.enabled)
        #expect(loaded.providers == [.plugin("i1"), .musicbrainz, .plugin("gone")])
        let connections = [fixture, other, searchOnly]
        #expect(loaded.providers.map { $0.label(connections) } == ["Owned fake", "MusicBrainz", "Unavailable saved provider"])
        #expect(loaded.providers.map { $0.detail(connections) } == ["Plugin · connected", "Built in", "Plugin · not connected"])
        #expect(loaded.available(connections) == [.plugin("i2")])
        #expect(loaded.canAdd)
    }

    @Test func savingSendsTheRevisionStateAndOrder() async throws {
        let fake = FakeServer(routes: ["PUT /api/v1/metadata/settings": .json(200, settings)])
        let preferences = MetadataProviders.Preferences(revision: 4, enabled: false, providers: [.musicbrainz, .plugin("i1")])
        _ = try await providers(fake).save(preferences)
        let body = try #require(fake.requests.first?.object)
        #expect(body["revision"] as? Int == 4)
        #expect(body["enabled"] as? Bool == false)
        let sent = try #require(body["providers"] as? [[String: Any]])
        #expect(sent.map { $0["kind"] as? String } == ["builtin_musicbrainz", "http_plugin"])
        #expect(sent[1]["installation_id"] as? String == "i1")
    }

    @Test func anOutdatedRevisionIsReported() async throws {
        let fake = FakeServer(routes: ["PUT /api/v1/metadata/settings": .json(409, #"{"error":"changed"}"#)])
        await #expect(throws: MetadataProviders.Problem.changed) { try await providers(fake).save(.init(revision: 1, enabled: true, providers: [.musicbrainz])) }
    }

    @Test func movingKeepsEveryProvider() {
        var preferences = MetadataProviders.Preferences(revision: 1, enabled: true, providers: [.musicbrainz, .plugin("i1"), .plugin("i2")])
        preferences.move(.plugin("i2"), by: -1)
        #expect(preferences.providers == [.musicbrainz, .plugin("i2"), .plugin("i1")])
        preferences.move(.musicbrainz, by: -1)
        #expect(preferences.providers == [.musicbrainz, .plugin("i2"), .plugin("i1")])
    }
}
