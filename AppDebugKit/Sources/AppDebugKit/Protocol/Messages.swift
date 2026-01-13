import Foundation

// MARK: - Request/Response Types

/// Incoming request from client
struct ProtocolRequest: Codable {
    let id: UInt64
    let method: String
    let params: [String: AnyCodable]?

    init(id: UInt64, method: String, params: [String: AnyCodable]? = nil) {
        self.id = id
        self.method = method
        self.params = params
    }
}

/// Outgoing response to client
struct ProtocolResponse: Codable {
    let id: UInt64
    let result: AnyCodable?
    let error: ProtocolError?

    static func success(id: UInt64, result: Any?) -> ProtocolResponse {
        ProtocolResponse(id: id, result: result.map { AnyCodable($0) }, error: nil)
    }

    static func error(id: UInt64, code: Int, message: String, data: Any? = nil) -> ProtocolResponse {
        ProtocolResponse(
            id: id,
            result: nil,
            error: ProtocolError(code: code, message: message, data: data.map { AnyCodable($0) })
        )
    }
}

/// Protocol error
struct ProtocolError: Codable {
    let code: Int
    let message: String
    let data: AnyCodable?
}

/// Outgoing event (no id, unsolicited)
struct ProtocolEvent: Codable {
    let method: String
    let params: AnyCodable?

    init(method: String, params: Any? = nil) {
        self.method = method
        self.params = params.map { AnyCodable($0) }
    }
}

// MARK: - Error Codes

enum ErrorCode {
    static let parseError = -32700
    static let invalidRequest = -32600
    static let methodNotFound = -32601
    static let invalidParams = -32602
    static let internalError = -32603
    static let elementNotFound = -1001
    static let actionFailed = -1002
    static let timeout = -1003
}

// MARK: - View Types

/// Represents a rectangle
struct Rect: Codable {
    let x: Double
    let y: Double
    let width: Double
    let height: Double

    init(_ rect: NSRect) {
        self.x = rect.origin.x
        self.y = rect.origin.y
        self.width = rect.size.width
        self.height = rect.size.height
    }
}

/// Represents a view node in the hierarchy
struct ViewNode: Codable {
    let id: String
    let type: String
    let className: String
    let frame: Rect
    let bounds: Rect
    let isHidden: Bool
    let isEnabled: Bool
    let accessibilityLabel: String?
    let accessibilityIdentifier: String?
    let properties: [String: AnyCodable]?
    let children: [ViewNode]?
}

/// Window information
struct WindowInfo: Codable {
    let id: String
    let title: String
    let frame: Rect
    let isKey: Bool
    let isMain: Bool
    let isVisible: Bool
}

// MARK: - AnyCodable Helper

/// Type-erased Codable wrapper for dynamic JSON values
struct AnyCodable: Codable {
    let value: Any

    init(_ value: Any) {
        self.value = value
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()

        if container.decodeNil() {
            self.value = NSNull()
        } else if let bool = try? container.decode(Bool.self) {
            self.value = bool
        } else if let int = try? container.decode(Int.self) {
            self.value = int
        } else if let double = try? container.decode(Double.self) {
            self.value = double
        } else if let string = try? container.decode(String.self) {
            self.value = string
        } else if let array = try? container.decode([AnyCodable].self) {
            self.value = array.map { $0.value }
        } else if let dict = try? container.decode([String: AnyCodable].self) {
            self.value = dict.mapValues { $0.value }
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "Unable to decode value")
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()

        switch value {
        case is NSNull:
            try container.encodeNil()
        case let bool as Bool:
            try container.encode(bool)
        case let int as Int:
            try container.encode(int)
        case let double as Double:
            try container.encode(double)
        case let string as String:
            try container.encode(string)
        case let array as [Any]:
            try container.encode(array.map { AnyCodable($0) })
        case let dict as [String: Any]:
            try container.encode(dict.mapValues { AnyCodable($0) })
        default:
            try container.encode(String(describing: value))
        }
    }
}
