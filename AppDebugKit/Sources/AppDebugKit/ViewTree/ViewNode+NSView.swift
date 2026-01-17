import AppKit
import Foundation

// MARK: - View Registry

/// Manages weak references to views and their IDs
@MainActor
final class ViewRegistry {
    static let shared = ViewRegistry()

    private var viewToId: [ObjectIdentifier: String] = [:]
    private var idToView: [String: WeakBox<NSView>] = [:]
    private var nextId: UInt64 = 1

    private init() {}

    /// Get or create an ID for a view
    func id(for view: NSView) -> String {
        let objectId = ObjectIdentifier(view)
        if let existingId = viewToId[objectId] {
            return existingId
        }

        let newId = "view-\(nextId)"
        nextId += 1
        viewToId[objectId] = newId
        idToView[newId] = WeakBox(view)
        return newId
    }

    /// Get a view by ID
    func view(for id: String) -> NSView? {
        guard let box = idToView[id] else { return nil }
        if let view = box.value {
            return view
        }
        // Clean up stale references from both maps
        if let staleObjectId = findObjectId(for: id) {
            viewToId.removeValue(forKey: staleObjectId)
        }
        idToView.removeValue(forKey: id)
        return nil
    }

    /// Find the ObjectIdentifier for a given ID (for cleanup)
    private func findObjectId(for id: String) -> ObjectIdentifier? {
        viewToId.first { $0.value == id }?.key
    }

    /// Clear all references (useful for testing)
    func reset() {
        viewToId.removeAll()
        idToView.removeAll()
        nextId = 1
    }

    /// Periodically clean up stale entries where the view has been deallocated
    func cleanupStaleEntries() {
        var staleIds: [String] = []
        for (id, box) in idToView {
            if box.value == nil {
                staleIds.append(id)
            }
        }
        for id in staleIds {
            if let objectId = findObjectId(for: id) {
                viewToId.removeValue(forKey: objectId)
            }
            idToView.removeValue(forKey: id)
        }
    }
}

/// Weak reference wrapper
private final class WeakBox<T: AnyObject> {
    weak var value: T?
    init(_ value: T) {
        self.value = value
    }
}

// MARK: - Window Registry

@MainActor
final class WindowRegistry {
    static let shared = WindowRegistry()

    private var windowToId: [ObjectIdentifier: String] = [:]
    private var idToWindow: [String: WeakBox<NSWindow>] = [:]
    private var nextId: UInt64 = 1

    private init() {}

    func id(for window: NSWindow) -> String {
        let objectId = ObjectIdentifier(window)
        if let existingId = windowToId[objectId] {
            return existingId
        }

        let newId = "window-\(nextId)"
        nextId += 1
        windowToId[objectId] = newId
        idToWindow[newId] = WeakBox(window)
        return newId
    }

    func window(for id: String) -> NSWindow? {
        guard let box = idToWindow[id] else { return nil }
        if let window = box.value {
            return window
        }
        // Clean up stale references from both maps
        if let staleObjectId = findObjectId(for: id) {
            windowToId.removeValue(forKey: staleObjectId)
        }
        idToWindow.removeValue(forKey: id)
        return nil
    }

    /// Find the ObjectIdentifier for a given ID (for cleanup)
    private func findObjectId(for id: String) -> ObjectIdentifier? {
        windowToId.first { $0.value == id }?.key
    }

    func reset() {
        windowToId.removeAll()
        idToWindow.removeAll()
        nextId = 1
    }

    /// Periodically clean up stale entries where the window has been deallocated
    func cleanupStaleEntries() {
        var staleIds: [String] = []
        for (id, box) in idToWindow {
            if box.value == nil {
                staleIds.append(id)
            }
        }
        for id in staleIds {
            if let objectId = findObjectId(for: id) {
                windowToId.removeValue(forKey: objectId)
            }
            idToWindow.removeValue(forKey: id)
        }
    }
}

// MARK: - View Tree Traversal

@MainActor
extension NSView {
    /// Convert this view to a ViewNode representation
    func toViewNode(depth: Int = -1, currentDepth: Int = 0) -> ViewNode {
        let viewId = ViewRegistry.shared.id(for: self)

        // Collect children if depth allows
        var childNodes: [ViewNode]? = nil
        if depth == -1 || currentDepth < depth {
            let children = self.subviews.map { subview in
                subview.toViewNode(depth: depth, currentDepth: currentDepth + 1)
            }
            if !children.isEmpty {
                childNodes = children
            }
        }

        // Extract common properties
        var properties: [String: Any] = [:]

        // Button properties
        if let button = self as? NSButton {
            properties["title"] = button.title
            properties["state"] = button.state.rawValue
            properties["isHighlighted"] = button.isHighlighted
        }

        // TextField properties
        if let textField = self as? NSTextField {
            properties["stringValue"] = textField.stringValue
            properties["placeholderString"] = textField.placeholderString ?? ""
            properties["isEditable"] = textField.isEditable
            properties["isSelectable"] = textField.isSelectable
        }

        // Control properties
        if let control = self as? NSControl {
            properties["isEnabled"] = control.isEnabled
        }

        return ViewNode(
            id: viewId,
            type: String(describing: type(of: self)),
            className: NSStringFromClass(type(of: self)),
            frame: Rect(self.frame),
            bounds: Rect(self.bounds),
            isHidden: self.isHidden,
            isEnabled: (self as? NSControl)?.isEnabled ?? true,
            accessibilityLabel: self.accessibilityLabel(),
            accessibilityIdentifier: self.accessibilityIdentifier(),
            properties: properties.isEmpty ? nil : properties.mapValues { AnyCodable($0) },
            children: childNodes
        )
    }

    /// Get the frame in screen coordinates
    var frameInScreen: NSRect {
        guard let window = self.window else { return frame }
        let frameInWindow = self.convert(self.bounds, to: nil)
        return window.convertToScreen(frameInWindow)
    }
}

// MARK: - Query Selectors

@MainActor
extension NSView {
    /// Find an element matching the selector
    func querySelector(_ selector: String) -> NSView? {
        return querySelectorAll(selector).first
    }

    /// Find all elements matching the selector
    func querySelectorAll(_ selector: String) -> [NSView] {
        var results: [NSView] = []
        querySelectRecursive(selector, results: &results)
        return results
    }

    private func querySelectRecursive(_ selector: String, results: inout [NSView]) {
        if matches(selector: selector) {
            results.append(self)
        }
        for subview in subviews {
            subview.querySelectRecursive(selector, results: &results)
        }
    }

    /// Check if this view matches the selector
    func matches(selector: String) -> Bool {
        let trimmed = selector.trimmingCharacters(in: .whitespaces)

        // #identifier - match accessibilityIdentifier
        if trimmed.hasPrefix("#") {
            let id = String(trimmed.dropFirst())
            return self.accessibilityIdentifier() == id
        }

        // .ClassName - match class name
        if trimmed.hasPrefix(".") {
            let className = String(trimmed.dropFirst())
            let actualClass = NSStringFromClass(type(of: self))
            return actualClass.contains(className) || String(describing: type(of: self)) == className
        }

        // [property=value] - match property
        if trimmed.hasPrefix("[") && trimmed.hasSuffix("]") {
            let inner = String(trimmed.dropFirst().dropLast())
            if let eqIndex = inner.firstIndex(of: "=") {
                let prop = String(inner[..<eqIndex])
                var value = String(inner[inner.index(after: eqIndex)...])
                // Remove quotes if present
                if value.hasPrefix("'") && value.hasSuffix("'") {
                    value = String(value.dropFirst().dropLast())
                }
                if value.hasPrefix("\"") && value.hasSuffix("\"") {
                    value = String(value.dropFirst().dropLast())
                }
                return matchesProperty(prop, value: value)
            }
        }

        // ClassName - match type name directly
        let actualClass = String(describing: type(of: self))
        return actualClass == trimmed || NSStringFromClass(type(of: self)).hasSuffix(trimmed)
    }

    private func matchesProperty(_ property: String, value: String) -> Bool {
        switch property {
        case "title":
            if let button = self as? NSButton {
                return button.title == value
            }
            return false
        case "identifier", "accessibilityIdentifier":
            return self.accessibilityIdentifier() == value
        case "label", "accessibilityLabel":
            return self.accessibilityLabel() == value
        case "value", "stringValue":
            if let textField = self as? NSTextField {
                return textField.stringValue == value
            }
            return false
        default:
            return false
        }
    }
}
