import Foundation

public struct ListeningProgress: Sendable {
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
        let kind = "progress_set"
        let recordingId: String
        let positionMs: Int
        let speed: Double

        enum CodingKeys: String, CodingKey {
            case kind, speed
            case recordingId = "recording_id"
            case positionMs = "position_ms"
        }
    }

    struct Receipt: Decodable { let acknowledged: [String] }

    let api: HitoAPI
    let device: String

    public init(api: HitoAPI, device: String) {
        self.api = api
        self.device = device
    }

    public func save(_ recordingId: String, position: Double, speed: Double) async throws {
        let operation = UUID().uuidString.lowercased()
        let action = Action(recordingId: recordingId, positionMs: Int((position * 1000).rounded(.down)), speed: speed)
        let receipt = try await api.post(["api", "v1", "sync"], body: Batch(deviceId: device, operations: [Operation(id: operation, action: action)]), as: Receipt.self)
        guard receipt.acknowledged.contains(where: { $0.lowercased() == operation }) else { throw Likes.Problem.notAcknowledged }
    }
}

public struct ProgressCheckpoint: Sendable {
    static let every: Double = 10
    private var saved: Double?

    public init() {}

    public mutating func observe(_ position: Double) -> Double? {
        guard position.isFinite, abs(position - (saved ?? 0)) >= Self.every else { return nil }
        saved = position
        return position
    }

    public mutating func settle(_ position: Double) -> Double? {
        guard position.isFinite, position > 0, position != saved else { return nil }
        saved = position
        return position
    }
}
