import Foundation
import Testing
@testable import HitoKit

@Suite struct PluginsTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let settings = #"{"credential_storage_ready":true,"installations":[{"id":"i1","service_id":"s1","revision":1,"checked_at":1,"credential_set":false,"health":{"state":"ok"},"manifest":{"id":"yt","name":"YouTube","protocol_version":1,"capabilities":["search","sources","metadata"]}},{"id":"i2","service_id":"s2","revision":1,"checked_at":1,"credential_set":false,"health":{"state":"unreachable","reason":"network"},"manifest":{"id":"sp","name":"Spotify public playlists","protocol_version":1,"capabilities":["playlists","artwork"]}}],"services":[{"id":"s1","name":"YouTube","base_url":"http://yt","revision":1},{"id":"s2","name":"Spotify public playlists","base_url":"http://sp","revision":1},{"id":"s3","name":"Lyrics","base_url":"http://ly","revision":1}]}"#

    func plugins(_ fake: FakeServer) -> Plugins {
        Plugins(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func connectionsListTheirCapabilitiesInTheDesignsWords() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/plugins": .json(200, settings)])
        let state = try await plugins(fake).load()
        #expect(state.connections.map(\.name) == ["YouTube", "Spotify public playlists"])
        #expect(state.connections.map(\.summary) == ["Search · Playback · Track details", "Playlists · Covers"])
        #expect(state.connections.map(\.health) == [.ok, .unreachable])
    }

    @Test func whatPluginsPowerNamesTheProviderForEachCapability() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/plugins": .json(200, settings)])
        let state = try await plugins(fake).load()
        #expect(state.powers.map(\.name) == ["Search", "Playback", "Track details", "Playlists", "Covers", "Lyrics"])
        #expect(state.powers.map(\.providers) == [["YouTube"], ["YouTube"], ["YouTube"], ["Spotify public playlists"], ["Spotify public playlists"], []])
    }

    @Test func servicesNotYetConnectedAreAvailable() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/plugins": .json(200, settings)])
        let state = try await plugins(fake).load()
        #expect(state.available.map(\.name) == ["Lyrics"])
    }
}
