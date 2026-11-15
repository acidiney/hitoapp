import Foundation
import Testing
@testable import HitoKit

@Suite struct ArtistReviewTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let context = #"{"account_id":"u1","recording_id":"r1","library_id":"l1","artist_id":"ar1","credited_name":"Band","source_id":null,"observation_id":"o1","artist_operation_id":null,"access_grant_id":"g1","metadata_binding":{"id":"b1","revision":2},"provider":{"kind":"http_plugin","installation_id":"i1"},"policy_revision":7,"connection":{"installation_revision":3,"service_id":"s1","service_revision":1}}"#

    func review(_ fake: FakeServer) -> ArtistReview {
        ArtistReview(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func providersAreNamedTheWayTheListenerKnowsThem() async throws {
        let on = FakeServer(routes: ["GET /api/v1/metadata/settings": .json(200, #"{"revision":4,"enabled":true,"providers":[{"kind":"builtin_musicbrainz"},{"kind":"http_plugin","installation_id":"i1"}],"automatic_artist_lookup":true,"automatic_artwork":true,"automatic_service_search":true}"#)])
        let providers = try await review(on).providers(named: ["i1": "Owned fake"])
        #expect(providers.map(\.name) == ["MusicBrainz", "Owned fake"])
        let off = FakeServer(routes: ["GET /api/v1/metadata/settings": .json(200, #"{"revision":4,"enabled":false,"providers":[{"kind":"builtin_musicbrainz"}],"automatic_artist_lookup":true,"automatic_artwork":true,"automatic_service_search":true}"#)])
        #expect(try await review(off).providers(named: [:]).isEmpty)
    }

    @Test func aSearchSendsTheReviewContextBackUnchanged() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/metadata/settings": .json(200, #"{"revision":4,"enabled":true,"providers":[{"kind":"http_plugin","installation_id":"i1"}],"automatic_artist_lookup":true,"automatic_artwork":true,"automatic_service_search":true}"#),
            "POST /api/v1/recordings/r1/artist-review/context": .json(200, context),
            "POST /api/v1/recordings/r1/artist-review/search": .json(200, #"{"status":"available","context":\#(context),"page":{"query":"Band","snapshot":null,"items":[{"artist":{"id":"p1","name":"Band"},"kind":"group","disambiguation":"from Luanda","country":"AO","begin_year":1999},{"artist":{"id":"p2","name":"Band Two"},"kind":null,"disambiguation":null}],"total_count":2,"next_cursor":null}}"#),
        ])
        let provider = try #require(try await review(fake).providers(named: ["i1": "Owned fake"]).first)
        let started = try await review(fake).start(recording: "r1", provider: provider)
        #expect(fake.requests[1].object?["provider"] as? [String: String] == ["kind": "http_plugin", "installation_id": "i1"])
        let (session, found) = try await review(fake).search(" Band ", in: started)
        #expect(found == [ArtistCandidate(id: "p1", name: "Band", detail: "Group · from Luanda · AO · 1999"), ArtistCandidate(id: "p2", name: "Band Two", detail: nil)])
        #expect(session == started)
        let body = try #require(fake.requests.last?.object)
        let sent = try JSONSerialization.data(withJSONObject: body["context"] as Any, options: .sortedKeys)
        let original = try JSONSerialization.data(withJSONObject: JSONSerialization.jsonObject(with: Data(context.utf8)), options: .sortedKeys)
        #expect(sent == original)
        #expect((body["search"] as? [String: Any])?["query"] as? String == "Band")
        #expect((body["search"] as? [String: Any])?["limit"] as? Int == 20)
    }

    @Test func aChosenArtistIsPreviewedThenConfirmed() async throws {
        let fake = FakeServer(routes: [
            "POST /api/v1/recordings/r1/artist-review/preview": .json(200, #"{"status":"available","context":\#(context),"profile":{"artist":{"id":"p1","name":"Band"},"sort_name":"Band, The","kind":"group","disambiguation":null,"revision":null}}"#),
            "POST /api/v1/recordings/r1/artist-review/confirm": .json(200, #"{"id":"as1"}"#),
        ])
        let session = ArtistReview.Session(recording: "r1", context: try JSONDecoder().decode(JSONValue.self, from: Data(context.utf8)))
        let candidate = ArtistCandidate(id: "p1", name: "Band", detail: nil)
        let (next, profile) = try await review(fake).preview(candidate, in: session)
        #expect(profile == ArtistProfile(name: "Band", detail: "Group"))
        try await review(fake).confirm(candidate, in: next)
        #expect(fake.requests.last?.object?["provider_artist_id"] as? String == "p1")
        #expect((fake.requests.last?.object?["context"] as? [String: Any])?["policy_revision"] as? Int == 7)
    }

    @Test func aReviewThatCantGoOnSaysWhy() async throws {
        let session = ArtistReview.Session(recording: "r1", context: .object([:]))
        let off = FakeServer(routes: ["POST /api/v1/recordings/r1/artist-review/search": .json(200, #"{"status":"disabled"}"#)])
        await #expect(throws: ArtistReview.Problem(message: "Track details from plugins are off in your settings.")) {
            try await review(off).search("Band", in: session)
        }
        let changed = FakeServer(routes: ["POST /api/v1/recordings/r1/artist-review/confirm": .json(409, #"{"error":"changed"}"#)])
        await #expect(throws: ArtistReview.Problem(message: "This track changed while you were choosing. Start again.")) {
            try await review(changed).confirm(ArtistCandidate(id: "p1", name: "Band", detail: nil), in: session)
        }
    }
}
