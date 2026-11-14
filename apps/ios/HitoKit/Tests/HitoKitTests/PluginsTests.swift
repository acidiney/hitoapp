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

    @Test func checkingAConnectionKeepsTheCredentialAndReportsAnUpdate() async throws {
        let installation = #"{"id":"i1","service_id":"s1","revision":2,"checked_at":5,"credential_set":false,"health":{"state":"ok"},"manifest":{"id":"yt","name":"YouTube","protocol_version":1,"capabilities":["search"]}}"#
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/refresh": .json(200, #"{"kind":"updated","installation":\#(installation)}"#)])
        let result = try await plugins(fake).check("i1", revision: 1)
        #expect(result == .updated(PluginConnection(id: "i1", name: "YouTube", capabilities: ["search"], health: .ok, revision: 2, checkedAt: Date(timeIntervalSince1970: 5))))
        let body = try #require(fake.requests.first?.object)
        #expect(body["revision"] as? Int == 1)
        #expect((body["credential"] as? [String: String]) == ["action": "keep"])
        #expect(body["accept_manifest_digest"] is NSNull)
    }

    @Test func aPluginAskingForMoreNamesWhatItWants() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/refresh": .json(200, #"{"kind":"review_required","digest":"d1","manifest":{"id":"yt","name":"YouTube","protocol_version":1,"capabilities":["search","artwork"]}}"#)])
        let result = try await plugins(fake).check("i1", revision: 1, had: ["search"])
        #expect(result == .review(digest: "d1", wants: ["Covers"]))
    }

    @Test func aPluginAskingToSignInNamesIt() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/refresh": .json(200, #"{"kind":"review_required","digest":"d2","manifest":{"id":"yt","name":"YouTube","protocol_version":1,"capabilities":["search"],"authorization":{"kind":"oauth2_pkce","client_id":"c","authorization_endpoint":"https://a","token_endpoint":"https://t","scopes":["read"]}}}"#)])
        #expect(try await plugins(fake).check("i1", revision: 1, had: ["search"]) == .review(digest: "d2", wants: ["Account sign-in"]))
        #expect(try await plugins(fake).check("i1", revision: 1, had: ["search"], signsIn: true) == .review(digest: "d2", wants: []))
    }

    @Test func acceptingAReviewSendsItsDigest() async throws {
        let installation = #"{"id":"i1","service_id":"s1","revision":3,"checked_at":5,"credential_set":false,"health":null,"manifest":{"id":"yt","name":"YouTube","protocol_version":1,"capabilities":["search","artwork"]}}"#
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/refresh": .json(200, #"{"kind":"updated","installation":\#(installation)}"#)])
        _ = try await plugins(fake).check("i1", revision: 2, accept: "d1")
        #expect(fake.requests.first?.object?["accept_manifest_digest"] as? String == "d1")
    }

    @Test func removingAPluginDeletesItsInstallation() async throws {
        let fake = FakeServer(routes: ["DELETE /api/v1/plugins/i1": .text(204, "")])
        try await plugins(fake).remove("i1")
        #expect(fake.requests.map(\.line) == ["DELETE /api/v1/plugins/i1"])
    }

    @Test func aPluginIsSearchedWithItsRevision() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/search": .json(200, #"{"items":[{"id":"p1","title":"Remote Song","artist":"Remote Band","duration_ms":200000}],"next_cursor":null}"#)])
        let found = try await plugins(fake).search("i1", revision: 2, query: "remote")
        #expect(found == [PluginResult(id: "p1", title: "Remote Song", artist: "Remote Band", durationMs: 200000)])
        let body = try #require(fake.requests.first?.object)
        #expect(body["installation_revision"] as? Int == 2)
        #expect(body["query"] as? String == "remote")
        #expect(body["limit"] as? Int == 10)
    }

    @Test func addingAResultAdoptsItIntoTheLibrary() async throws {
        let recording = #"{"id":"r9","library_id":"l1","title":"Remote Song","inherited_title":"Remote Song","inherited_artist":"Remote Band","artist":"Remote Band","duration_ms":200000,"available":false,"plays_from_services":true,"liked":false,"album":null,"release_id":null}"#
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/recordings": .json(200, recording)])
        let track = try await plugins(fake).adopt("i1", revision: 2, result: "p1")
        #expect(track.id == "r9" && track.playsFromServices)
        #expect(fake.requests.first?.object?["provider_recording_id"] as? String == "p1")
    }

    @Test func aPluginThatSignsInToAnAccountSaysSo() async throws {
        let signing = #"{"credential_storage_ready":true,"installations":[{"id":"i3","service_id":"s3","revision":1,"checked_at":1,"credential_set":false,"health":{"state":"ok"},"manifest":{"id":"sp","name":"Spotify","protocol_version":1,"capabilities":["playlists"],"authorization":{"kind":"oauth2_pkce","client_id":"c","authorization_endpoint":"https://a","token_endpoint":"https://t","scopes":["read"]}}}],"services":[]}"#
        let fake = FakeServer(routes: ["GET /api/v1/plugins": .json(200, signing)])
        let state = try await plugins(fake).load()
        #expect(state.connections.map(\.signsIn) == [true])
        let plain = try await plugins(FakeServer(routes: ["GET /api/v1/plugins": .json(200, settings)])).load()
        #expect(plain.connections.map(\.signsIn) == [false, false])
    }

    @Test func accountSignInReadsTheConnectionState() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/plugins/i3/connection": .json(200, #"{"status":"needs_reauthorization","scopes":["read"],"expires_at":null}"#)])
        #expect(try await plugins(fake).account("i3") == .needsSignInAgain)
        let connected = FakeServer(routes: ["GET /api/v1/plugins/i3/connection": .json(200, #"{"status":"connected","scopes":["read"],"expires_at":9}"#)])
        #expect(try await plugins(connected).account("i3") == .connected)
        let none = FakeServer(routes: ["GET /api/v1/plugins/i3/connection": .json(200, #"{"status":"not_connected","scopes":[]}"#)])
        #expect(try await plugins(none).account("i3") == .notConnected)
    }

    @Test func signingOutOfTheAccountKeepsThePlugin() async throws {
        let fake = FakeServer(routes: ["DELETE /api/v1/plugins/i3/connection": .text(204, "")])
        try await plugins(fake).signOut("i3")
        #expect(fake.requests.map(\.line) == ["DELETE /api/v1/plugins/i3/connection"])
    }
}
