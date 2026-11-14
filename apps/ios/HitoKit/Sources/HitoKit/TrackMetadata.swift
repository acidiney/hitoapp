import Foundation

public struct TrackMetadata: Sendable {
    public enum Field: String, Sendable {
        case title, artist
    }

    public struct Value: Equatable, Sendable {
        public let value: String
        public let source: String?
        public let personal: String?

        public init(value: String, source: String?, personal: String?) {
            self.value = value
            self.source = source
            self.personal = personal
        }

        public func note(field: String) -> String {
            "\(personal == nil ? "Following source metadata" : "Personal \(field)") · Source \(field): \(source ?? "Not provided by source")"
        }
    }

    public struct Credit: Equatable, Sendable {
        public let name: String
        public let join: String?

        public init(name: String, join: String?) {
            self.name = name
            self.join = join
        }
    }

    public struct Details: Equatable, Sendable {
        public let title: Value
        public let artist: Value
        public let facts: [String]
        public let musicBrainz: String?
        public let credits: String?
    }

    public static func creditLine(_ credits: [Credit]) -> String? {
        guard !credits.isEmpty else { return nil }
        return credits.enumerated().map { index, credit in
            credit.name + (index + 1 < credits.count ? (credit.join ?? ", ") : "")
        }.joined()
    }

    struct Wire: Decodable {
        struct Text: Decodable {
            let value: String
            let inherited: String?
            let personal: String?
        }

        struct Facts: Decodable {
            let year: Int?
            let label: String?
        }

        struct Format: Decodable {
            let format: String?
            let bitDepth: Int?
            let sampleRateHz: Int?

            enum CodingKeys: String, CodingKey {
                case format
                case bitDepth = "bit_depth"
                case sampleRateHz = "sample_rate_hz"
            }
        }

        struct Observation: Decodable { let album: String? }

        struct Credited: Decodable {
            let name: String
            let join: String?
        }

        let title: Text
        let artist: Text
        let musicbrainzRecordingId: String?
        let releaseFacts: Facts?
        let audioFormat: Format?
        let selectedObservation: Observation?
        let creditedArtists: [Credited]?

        enum CodingKeys: String, CodingKey {
            case title, artist
            case creditedArtists = "credited_artists"
            case musicbrainzRecordingId = "musicbrainz_recording_id"
            case releaseFacts = "release_facts"
            case audioFormat = "audio_format"
            case selectedObservation = "selected_observation"
        }
    }

    struct Batch: Encodable {
        let deviceId: String
        let operations: [Operation]

        enum CodingKeys: String, CodingKey {
            case deviceId = "device_id"
            case operations
        }
    }

    struct Operation: Encodable {
        let id: String
        let action: Action
    }

    struct Action: Encodable {
        let kind: String
        let recordingId: String
        let title: String?
        let artist: String?

        enum CodingKeys: String, CodingKey {
            case kind, title, artist
            case recordingId = "recording_id"
        }
    }

    struct Receipt: Decodable { let acknowledged: [String] }

    let api: HitoAPI
    let device: String

    public init(api: HitoAPI, device: String) {
        self.api = api
        self.device = device
    }

    public func load(_ recording: String) async throws -> Details {
        let wire = try await api.get(["api", "v1", "recordings", recording, "metadata"], as: Wire.self)
        let format = wire.audioFormat.map { format in
            [format.format?.uppercased(), format.bitDepth.map { "\($0)-bit" }, format.sampleRateHz.map(Self.rate)].compactMap { $0 }.joined(separator: " · ")
        }
        let facts = [wire.selectedObservation?.album, wire.releaseFacts?.year.map(String.init), wire.releaseFacts?.label, format].compactMap { $0 }.filter { !$0.isEmpty }
        return Details(
            title: Value(value: wire.title.value, source: wire.title.inherited, personal: wire.title.personal),
            artist: Value(value: wire.artist.value, source: wire.artist.inherited, personal: wire.artist.personal),
            facts: facts,
            musicBrainz: wire.musicbrainzRecordingId,
            credits: Self.creditLine((wire.creditedArtists ?? []).map { Credit(name: $0.name, join: $0.join) })
        )
    }

    public func save(_ recording: String, field: Field, personal: String?) async throws {
        let kind = "recording_\(field.rawValue)_\(personal == nil ? "inherit" : "set")"
        let action = Action(kind: kind, recordingId: recording, title: field == .title ? personal : nil, artist: field == .artist ? personal : nil)
        let operation = UUID().uuidString.lowercased()
        let receipt = try await api.post(["api", "v1", "sync"], body: Batch(deviceId: device, operations: [Operation(id: operation, action: action)]), as: Receipt.self)
        guard receipt.acknowledged.contains(where: { $0.lowercased() == operation }) else { throw Likes.Problem.notAcknowledged }
    }

    static func rate(_ hertz: Int) -> String {
        let kilo = Double(hertz) / 1000
        return kilo.rounded() == kilo ? "\(Int(kilo)) kHz" : "\(kilo) kHz"
    }
}
