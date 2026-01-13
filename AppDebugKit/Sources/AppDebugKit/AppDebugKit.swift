import Foundation
import AppKit

/// Configuration for the debug server
public struct DebugServerConfig: Sendable {
    /// Port to listen on (default: 9222)
    public var port: UInt16

    /// Hostname to bind to (default: "127.0.0.1")
    public var host: String

    /// Maximum view tree depth to traverse (-1 for unlimited)
    public var maxTreeDepth: Int

    public init(
        port: UInt16 = 9222,
        host: String = "127.0.0.1",
        maxTreeDepth: Int = 50
    ) {
        self.port = port
        self.host = host
        self.maxTreeDepth = maxTreeDepth
    }

    /// Default config for debug builds
    public static var debug: DebugServerConfig {
        DebugServerConfig()
    }
}

/// Main entry point for the debug server
@MainActor
public final class AppDebugKit {

    /// Shared instance
    public static let shared = AppDebugKit()

    /// Whether the server is currently running
    public private(set) var isRunning: Bool = false

    /// Current configuration
    public private(set) var config: DebugServerConfig?

    /// The port the server is listening on
    public var port: UInt16? { server?.port }

    private var server: DebugServer?

    private init() {}

    /// Start the debug server with the given configuration
    /// - Parameter config: Server configuration
    /// - Throws: If the server fails to start
    public func start(config: DebugServerConfig = .debug) throws {
        guard !isRunning else { return }

        let server = DebugServer(config: config)
        try server.start()

        self.server = server
        self.config = config
        self.isRunning = true

        print("[AppDebugKit] Server started on ws://\(config.host):\(config.port)")
    }

    /// Stop the debug server
    public func stop() {
        guard isRunning else { return }

        server?.stop()
        server = nil
        config = nil
        isRunning = false

        print("[AppDebugKit] Server stopped")
    }
}

// MARK: - SwiftUI Convenience

#if canImport(SwiftUI)
import SwiftUI

public extension View {
    /// Register this view with a debug identifier for easier querying
    func debugIdentifier(_ identifier: String) -> some View {
        self.accessibilityIdentifier(identifier)
    }
}
#endif
