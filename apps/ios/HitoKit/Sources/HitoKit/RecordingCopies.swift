import Foundation

public struct RecordingCopies: Sendable {
    public struct Copy: Equatable, Identifiable, Sendable {
        public let id: String
        public let title: String
        public let line: String
        public let audio: String
        public let available: Bool
        public let proof: String
        public let sources: [String]
        public let canSeparate: Bool
    }

    struct Wire: Decodable {
        struct Item: Decodable {
            struct Source: Decodable {
                let label: String
                let ready: Bool
            }

            let recordingId: String
            let title: String
            let artist: String
            let album: String?
            let available: Bool
            let canonical: Bool
            let proof: String?
            let sources: [Source]

            enum CodingKeys: String, CodingKey {
                case title, artist, album, available, canonical, proof, sources
                case recordingId = "recording_id"
            }
        }

        let copies: [Item]
    }

    struct Split: Decodable { let separate: Bool }

    static let proofs = ["musicbrainz_recording": "Same MusicBrainz recording in its tags", "isrc": "Same ISRC"]

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func load(_ recording: String) async throws -> [Copy] {
        try await api.get(["api", "v1", "recordings", recording, "copies"], as: Wire.self).copies.map { item in
            Copy(
                id: item.recordingId,
                title: item.title,
                line: [item.artist, item.album].compactMap { $0 }.joined(separator: " · "),
                audio: item.available ? "Audio ready" : "No audio ready",
                available: item.available,
                proof: item.proof.flatMap { Self.proofs[$0] } ?? "Shown in your lists",
                sources: item.sources.map { $0.ready ? "\($0.label) · ready" : $0.label },
                canSeparate: !item.canonical
            )
        }
    }

    public func separate(_ copy: String) async throws -> Bool {
        try await api.post(["api", "v1", "recordings", copy, "copies", "split"], body: [String: String](), as: Split.self).separate
    }
}
