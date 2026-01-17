import AppKit
import Foundation
import Network

/// Errors that can occur when starting or running the debug server
enum DebugServerError: Error, LocalizedError {
    case invalidPort(UInt16)

    var errorDescription: String? {
        switch self {
        case .invalidPort(let port):
            return "Invalid port number: \(port). Port must be between 1 and 65535."
        }
    }
}

/// WebSocket-based debug server
@MainActor
final class DebugServer {
    let config: DebugServerConfig
    var port: UInt16 { config.port }

    private var listener: NWListener?
    private var connections: [NWConnection] = []

    init(config: DebugServerConfig) {
        self.config = config
    }

    func start() throws {
        let parameters = NWParameters.tcp
        parameters.allowLocalEndpointReuse = true

        // Bind to specific host interface for security
        parameters.requiredLocalEndpoint = NWEndpoint.hostPort(
            host: NWEndpoint.Host(config.host),
            port: NWEndpoint.Port(rawValue: config.port) ?? .any
        )

        // Create WebSocket options
        let wsOptions = NWProtocolWebSocket.Options()
        wsOptions.autoReplyPing = true
        parameters.defaultProtocolStack.applicationProtocols.insert(wsOptions, at: 0)

        // Note: Don't pass port to NWListener when requiredLocalEndpoint is set
        // as it would conflict and cause "Invalid argument" error
        listener = try NWListener(using: parameters)
        listener?.stateUpdateHandler = { [weak self] state in
            switch state {
            case .ready:
                print("[AppDebugKit] Listening on \(self?.config.host ?? "?"):\(self?.config.port ?? 0)")
            case .failed(let error):
                print("[AppDebugKit] Listener failed: \(error)")
            default:
                break
            }
        }

        listener?.newConnectionHandler = { [weak self] connection in
            Task { @MainActor in
                self?.handleNewConnection(connection)
            }
        }

        listener?.start(queue: .main)
    }

    func stop() {
        listener?.cancel()
        listener = nil
        for connection in connections {
            connection.cancel()
        }
        connections.removeAll()
    }

    private func handleNewConnection(_ connection: NWConnection) {
        connections.append(connection)
        print("[AppDebugKit] Client connected")

        connection.stateUpdateHandler = { [weak self, weak connection] state in
            Task { @MainActor in
                guard let connection = connection else { return }
                switch state {
                case .ready:
                    self?.receiveMessage(on: connection)
                case .failed, .cancelled:
                    self?.connections.removeAll { $0 === connection }
                    print("[AppDebugKit] Client disconnected")
                default:
                    break
                }
            }
        }

        connection.start(queue: .main)
    }

    private func receiveMessage(on connection: NWConnection) {
        connection.receiveMessage { [weak self] content, context, isComplete, error in
            Task { @MainActor in
                guard let self = self else { return }

                if let error = error {
                    print("[AppDebugKit] Receive error: \(error)")
                    return
                }

                if let content = content, !content.isEmpty {
                    self.handleMessage(content, on: connection)
                }

                // Continue receiving
                self.receiveMessage(on: connection)
            }
        }
    }

    private func handleMessage(_ data: Data, on connection: NWConnection) {
        guard String(data: data, encoding: .utf8) != nil else {
            sendError(on: connection, id: 0, code: ErrorCode.parseError, message: "Invalid UTF-8")
            return
        }

        guard let request = try? JSONDecoder().decode(ProtocolRequest.self, from: data) else {
            sendError(on: connection, id: 0, code: ErrorCode.parseError, message: "Invalid JSON")
            return
        }

        // Route to handler
        let response = handleRequest(request)
        sendResponse(response, on: connection)
    }

    private func handleRequest(_ request: ProtocolRequest) -> ProtocolResponse {
        let params = request.params ?? [:]

        switch request.method {
        // Runtime domain
        case "Runtime.getInfo":
            return handleRuntimeGetInfo(id: request.id)

        // View domain
        case "View.getTree":
            return handleViewGetTree(id: request.id, params: params)
        case "View.querySelector":
            return handleViewQuerySelector(id: request.id, params: params)
        case "View.querySelectorAll":
            return handleViewQuerySelectorAll(id: request.id, params: params)
        case "View.getElement":
            return handleViewGetElement(id: request.id, params: params)
        case "View.getScreenshot":
            return handleViewGetScreenshot(id: request.id, params: params)

        // Input domain
        case "Input.click":
            return handleInputClick(id: request.id, params: params)
        case "Input.clickAt":
            return handleInputClickAt(id: request.id, params: params)
        case "Input.type":
            return handleInputType(id: request.id, params: params)
        case "Input.pressKey":
            return handleInputPressKey(id: request.id, params: params)
        case "Input.focus":
            return handleInputFocus(id: request.id, params: params)
        case "Input.scroll":
            return handleInputScroll(id: request.id, params: params)

        // Window domain
        case "Window.list":
            return handleWindowList(id: request.id)
        case "Window.focus":
            return handleWindowFocus(id: request.id, params: params)
        case "Window.resize":
            return handleWindowResize(id: request.id, params: params)

        default:
            return .error(id: request.id, code: ErrorCode.methodNotFound, message: "Unknown method: \(request.method)")
        }
    }

    // MARK: - Runtime Domain

    private func handleRuntimeGetInfo(id: UInt64) -> ProtocolResponse {
        let bundle = Bundle.main
        let info: [String: Any] = [
            "appName": bundle.infoDictionary?["CFBundleName"] as? String ?? "Unknown",
            "bundleId": bundle.bundleIdentifier ?? "unknown",
            "version": bundle.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0.0.0",
            "protocolVersion": "1.0",
            "pid": ProcessInfo.processInfo.processIdentifier,
            "mainWindowId": NSApp.mainWindow.map { WindowRegistry.shared.id(for: $0) } as Any
        ]
        return .success(id: id, result: info)
    }

    // MARK: - View Domain

    /// Get the default root view, preferring keyWindow, then mainWindow, then any visible window
    private func getDefaultRootView() -> NSView? {
        if let keyWindow = NSApp.keyWindow, let contentView = keyWindow.contentView {
            return contentView
        }
        if let mainWindow = NSApp.mainWindow, let contentView = mainWindow.contentView {
            return contentView
        }
        // Fall back to first visible window (excluding panels)
        for window in NSApp.windows {
            if window.isVisible && !window.isKind(of: NSPanel.self), let contentView = window.contentView {
                return contentView
            }
        }
        return nil
    }

    private func handleViewGetTree(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        // Clean up stale registry entries periodically
        ViewRegistry.shared.cleanupStaleEntries()
        WindowRegistry.shared.cleanupStaleEntries()

        let depth = (params["depth"]?.value as? Int) ?? 5
        let rootId = params["rootId"]?.value as? String

        let rootView: NSView?
        if let rootId = rootId {
            rootView = ViewRegistry.shared.view(for: rootId)
        } else {
            rootView = getDefaultRootView()
        }

        guard let view = rootView else {
            return .error(id: id, code: ErrorCode.elementNotFound, message: "No root view found")
        }

        let tree = view.toViewNode(depth: depth)
        return .success(id: id, result: ["root": tree])
    }

    private func handleViewQuerySelector(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        // Clean up stale registry entries
        ViewRegistry.shared.cleanupStaleEntries()

        guard let selector = params["selector"]?.value as? String else {
            return .error(id: id, code: ErrorCode.invalidParams, message: "Missing selector parameter")
        }

        let rootView: NSView?
        if let rootId = params["rootId"]?.value as? String {
            rootView = ViewRegistry.shared.view(for: rootId)
        } else {
            rootView = getDefaultRootView()
        }

        guard let view = rootView else {
            return .error(id: id, code: ErrorCode.elementNotFound, message: "No root view found")
        }

        if let found = view.querySelector(selector) {
            let elementId = ViewRegistry.shared.id(for: found)
            return .success(id: id, result: ["elementId": elementId])
        }

        return .success(id: id, result: ["elementId": NSNull()])
    }

    private func handleViewQuerySelectorAll(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        // Clean up stale registry entries
        ViewRegistry.shared.cleanupStaleEntries()

        guard let selector = params["selector"]?.value as? String else {
            return .error(id: id, code: ErrorCode.invalidParams, message: "Missing selector parameter")
        }

        let rootView: NSView?
        if let rootId = params["rootId"]?.value as? String {
            rootView = ViewRegistry.shared.view(for: rootId)
        } else {
            rootView = getDefaultRootView()
        }

        guard let view = rootView else {
            return .error(id: id, code: ErrorCode.elementNotFound, message: "No root view found")
        }

        let found = view.querySelectorAll(selector)
        let elementIds = found.map { ViewRegistry.shared.id(for: $0) }
        return .success(id: id, result: ["elementIds": elementIds])
    }

    private func handleViewGetElement(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        guard let elementId = params["elementId"]?.value as? String else {
            return .error(id: id, code: ErrorCode.invalidParams, message: "Missing elementId parameter")
        }

        guard let view = ViewRegistry.shared.view(for: elementId) else {
            return .error(id: id, code: ErrorCode.elementNotFound, message: "Element not found: \(elementId)")
        }

        let node = view.toViewNode(depth: 0)
        return .success(id: id, result: node)
    }

    private func handleViewGetScreenshot(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        let targetView: NSView?

        if let elementId = params["elementId"]?.value as? String {
            targetView = ViewRegistry.shared.view(for: elementId)
        } else {
            targetView = getDefaultRootView()
        }

        guard let view = targetView else {
            return .error(id: id, code: ErrorCode.elementNotFound, message: "No view to capture")
        }

        // Capture the view to an image
        guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
            return .error(id: id, code: ErrorCode.actionFailed, message: "Failed to create bitmap")
        }

        view.cacheDisplay(in: view.bounds, to: bitmap)

        let format = (params["format"]?.value as? String) ?? "png"
        let quality = (params["quality"]?.value as? Int) ?? 80

        let imageData: Data?
        if format == "jpeg" {
            imageData = bitmap.representation(using: .jpeg, properties: [.compressionFactor: Double(quality) / 100.0])
        } else {
            imageData = bitmap.representation(using: .png, properties: [:])
        }

        guard let data = imageData else {
            return .error(id: id, code: ErrorCode.actionFailed, message: "Failed to encode image")
        }

        let base64 = data.base64EncodedString()
        return .success(id: id, result: [
            "data": base64,
            "width": Int(view.bounds.width),
            "height": Int(view.bounds.height)
        ])
    }

    // MARK: - Input Domain

    private func handleInputClick(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        // Clean up stale registry entries
        ViewRegistry.shared.cleanupStaleEntries()

        guard let elementId = params["elementId"]?.value as? String else {
            return .error(id: id, code: ErrorCode.invalidParams, message: "Missing elementId parameter")
        }

        guard let view = ViewRegistry.shared.view(for: elementId) else {
            return .error(id: id, code: ErrorCode.elementNotFound, message: "Element not found: \(elementId)")
        }

        let clickCount = (params["clickCount"]?.value as? Int) ?? 1

        if InputSimulator.click(view: view, clickCount: clickCount) {
            return .success(id: id, result: ["success": true])
        } else {
            return .error(id: id, code: ErrorCode.actionFailed, message: "Click failed")
        }
    }

    private func handleInputClickAt(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        guard let x = params["x"]?.value as? Double,
              let y = params["y"]?.value as? Double else {
            return .error(id: id, code: ErrorCode.invalidParams, message: "Missing x or y parameter")
        }

        guard let window = NSApp.keyWindow ?? NSApp.mainWindow else {
            return .error(id: id, code: ErrorCode.actionFailed, message: "No window available")
        }

        let clickCount = (params["clickCount"]?.value as? Int) ?? 1

        if InputSimulator.clickAt(window: window, x: x, y: y, clickCount: clickCount) {
            return .success(id: id, result: ["success": true])
        } else {
            return .error(id: id, code: ErrorCode.actionFailed, message: "Click failed")
        }
    }

    private func handleInputType(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        guard let text = params["text"]?.value as? String else {
            return .error(id: id, code: ErrorCode.invalidParams, message: "Missing text parameter")
        }

        let targetView: NSView?
        if let elementId = params["elementId"]?.value as? String {
            targetView = ViewRegistry.shared.view(for: elementId)
        } else {
            targetView = nil
        }

        let clearFirst = (params["clearFirst"]?.value as? Bool) ?? false

        if InputSimulator.type(text: text, into: targetView, clearFirst: clearFirst) {
            return .success(id: id, result: ["success": true])
        } else {
            return .error(id: id, code: ErrorCode.actionFailed, message: "Type failed")
        }
    }

    private func handleInputPressKey(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        guard let key = params["key"]?.value as? String else {
            return .error(id: id, code: ErrorCode.invalidParams, message: "Missing key parameter")
        }

        let modifiers: [String]
        if let mods = params["modifiers"]?.value as? [Any] {
            modifiers = mods.compactMap { $0 as? String }
        } else {
            modifiers = []
        }

        if InputSimulator.pressKey(key, modifiers: modifiers) {
            return .success(id: id, result: ["success": true])
        } else {
            return .error(id: id, code: ErrorCode.actionFailed, message: "Key press failed")
        }
    }

    private func handleInputFocus(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        guard let elementId = params["elementId"]?.value as? String else {
            return .error(id: id, code: ErrorCode.invalidParams, message: "Missing elementId parameter")
        }

        guard let view = ViewRegistry.shared.view(for: elementId) else {
            return .error(id: id, code: ErrorCode.elementNotFound, message: "Element not found: \(elementId)")
        }

        if InputSimulator.focus(view: view) {
            return .success(id: id, result: ["success": true])
        } else {
            return .error(id: id, code: ErrorCode.actionFailed, message: "Focus failed")
        }
    }

    private func handleInputScroll(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        guard let elementId = params["elementId"]?.value as? String else {
            return .error(id: id, code: ErrorCode.invalidParams, message: "Missing elementId parameter")
        }

        guard let view = ViewRegistry.shared.view(for: elementId) else {
            return .error(id: id, code: ErrorCode.elementNotFound, message: "Element not found: \(elementId)")
        }

        let deltaX = (params["deltaX"]?.value as? Double) ?? 0
        let deltaY = (params["deltaY"]?.value as? Double) ?? 0

        if InputSimulator.scroll(view: view, deltaX: deltaX, deltaY: deltaY) {
            return .success(id: id, result: ["success": true])
        } else {
            return .error(id: id, code: ErrorCode.actionFailed, message: "Scroll failed")
        }
    }

    // MARK: - Window Domain

    private func handleWindowList(id: UInt64) -> ProtocolResponse {
        let windows = NSApp.windows.filter { !$0.isKind(of: NSPanel.self) }.map { window -> WindowInfo in
            WindowInfo(
                id: WindowRegistry.shared.id(for: window),
                title: window.title,
                frame: Rect(window.frame),
                isKey: window.isKeyWindow,
                isMain: window.isMainWindow,
                isVisible: window.isVisible
            )
        }
        return .success(id: id, result: ["windows": windows])
    }

    private func handleWindowFocus(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        guard let windowId = params["windowId"]?.value as? String else {
            return .error(id: id, code: ErrorCode.invalidParams, message: "Missing windowId parameter")
        }

        guard let window = WindowRegistry.shared.window(for: windowId) else {
            return .error(id: id, code: ErrorCode.elementNotFound, message: "Window not found: \(windowId)")
        }

        window.makeKeyAndOrderFront(nil)
        return .success(id: id, result: ["success": true])
    }

    private func handleWindowResize(id: UInt64, params: [String: AnyCodable]) -> ProtocolResponse {
        guard let windowId = params["windowId"]?.value as? String else {
            return .error(id: id, code: ErrorCode.invalidParams, message: "Missing windowId parameter")
        }

        guard let width = params["width"]?.value as? Double,
              let height = params["height"]?.value as? Double else {
            return .error(id: id, code: ErrorCode.invalidParams, message: "Missing width or height parameter")
        }

        guard let window = WindowRegistry.shared.window(for: windowId) else {
            return .error(id: id, code: ErrorCode.elementNotFound, message: "Window not found: \(windowId)")
        }

        var frame = window.frame
        frame.size = NSSize(width: width, height: height)
        window.setFrame(frame, display: true)

        return .success(id: id, result: ["success": true])
    }

    // MARK: - Send Helpers

    private func sendResponse(_ response: ProtocolResponse, on connection: NWConnection) {
        guard let data = try? JSONEncoder().encode(response) else { return }

        // Create WebSocket frame
        let metadata = NWProtocolWebSocket.Metadata(opcode: .text)
        let context = NWConnection.ContentContext(identifier: "response", metadata: [metadata])

        connection.send(content: data, contentContext: context, isComplete: true, completion: .idempotent)
    }

    private func sendError(on connection: NWConnection, id: UInt64, code: Int, message: String) {
        let response = ProtocolResponse.error(id: id, code: code, message: message)
        sendResponse(response, on: connection)
    }
}
