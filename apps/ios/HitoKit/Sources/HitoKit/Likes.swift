import Foundation

public struct Likes: Sendable {
    public enum Problem: Error, Equatable {
        case notAcknowledged
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
        let kind = "like_set"
        let recordingId: String
        let liked: Bool

        enum CodingKeys: String, CodingKey {
            case kind, liked
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

    public func setLiked(_ recordingId: String, _ liked: Bool) async throws {
        let operation = UUID().uuidString.lowercased()
        let batch = Batch(deviceId: device, operations: [Operation(id: operation, action: Action(recordingId: recordingId, liked: liked))])
        let receipt = try await api.post(["api", "v1", "sync"], body: batch, as: Receipt.self)
        guard receipt.acknowledged.contains(where: { $0.lowercased() == operation }) else { throw Problem.notAcknowledged }
    }
}
