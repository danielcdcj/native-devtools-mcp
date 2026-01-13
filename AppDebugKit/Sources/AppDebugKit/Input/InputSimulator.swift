import AppKit
import Foundation

/// Simulates user input on views without using system-level events
@MainActor
enum InputSimulator {

    // MARK: - Click

    enum MouseButton {
        case left
        case right
    }

    /// Click on a view
    static func click(view: NSView, clickCount: Int = 1, mouseButton: MouseButton = .left) -> Bool {
        guard let window = view.window else { return false }

        // Calculate click point (center of view)
        let boundsCenter = NSPoint(x: view.bounds.midX, y: view.bounds.midY)
        let windowPoint = view.convert(boundsCenter, to: nil)

        // Create mouse events
        let timestamp = ProcessInfo.processInfo.systemUptime

        // For buttons and controls, try direct action first
        if let button = view as? NSButton {
            button.performClick(nil)
            return true
        }

        if let control = view as? NSControl, let action = control.action, let target = control.target {
            NSApp.sendAction(action, to: target, from: control)
            return true
        }

        // Otherwise simulate mouse events
        for _ in 0..<clickCount {
            // Mouse down
            if let mouseDown = NSEvent.mouseEvent(
                with: mouseButton == .left ? .leftMouseDown : .rightMouseDown,
                location: windowPoint,
                modifierFlags: [],
                timestamp: timestamp,
                windowNumber: window.windowNumber,
                context: nil,
                eventNumber: 0,
                clickCount: clickCount,
                pressure: 1.0
            ) {
                view.mouseDown(with: mouseDown)
            }

            // Mouse up
            if let mouseUp = NSEvent.mouseEvent(
                with: mouseButton == .left ? .leftMouseUp : .rightMouseUp,
                location: windowPoint,
                modifierFlags: [],
                timestamp: timestamp + 0.05,
                windowNumber: window.windowNumber,
                context: nil,
                eventNumber: 0,
                clickCount: clickCount,
                pressure: 0.0
            ) {
                view.mouseUp(with: mouseUp)
            }
        }

        return true
    }

    /// Click at coordinates relative to window
    static func clickAt(window: NSWindow, x: Double, y: Double, clickCount: Int = 1) -> Bool {
        let point = NSPoint(x: x, y: y)
        let timestamp = ProcessInfo.processInfo.systemUptime

        // Find the view at this point
        if let contentView = window.contentView,
           let targetView = contentView.hitTest(point) {
            return click(view: targetView, clickCount: clickCount)
        }

        return false
    }

    // MARK: - Type Text

    /// Get the default window, preferring keyWindow, then mainWindow, then any visible window
    private static func getDefaultWindow() -> NSWindow? {
        if let keyWindow = NSApp.keyWindow {
            return keyWindow
        }
        if let mainWindow = NSApp.mainWindow {
            return mainWindow
        }
        // Fall back to first visible window (excluding panels)
        for window in NSApp.windows {
            if window.isVisible && !window.isKind(of: NSPanel.self) {
                return window
            }
        }
        return nil
    }

    /// Type text into a view (must be a text input)
    static func type(text: String, into view: NSView?, clearFirst: Bool = false) -> Bool {
        // Find the text input view
        let targetView: NSView?
        if let view = view {
            targetView = view
        } else if let window = getDefaultWindow() {
            targetView = window.firstResponder as? NSView
        } else {
            return false
        }

        guard let target = targetView else { return false }

        // Handle NSTextField
        if let textField = target as? NSTextField {
            if clearFirst {
                textField.stringValue = ""
            }
            textField.stringValue += text
            return true
        }

        // Handle NSTextView
        if let textView = target as? NSTextView {
            if clearFirst {
                textView.string = ""
            }
            textView.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
            return true
        }

        // Try to find a text field in the responder chain
        var responder = target.window?.firstResponder
        while let current = responder {
            if let textField = current as? NSTextField {
                if clearFirst {
                    textField.stringValue = ""
                }
                textField.stringValue += text
                return true
            }
            if let textView = current as? NSTextView {
                if clearFirst {
                    textView.string = ""
                }
                textView.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
                return true
            }
            responder = current.nextResponder
        }

        return false
    }

    // MARK: - Press Key

    /// Press a key combination
    static func pressKey(_ key: String, modifiers: [String] = []) -> Bool {
        guard let window = getDefaultWindow() else { return false }

        var modifierFlags: NSEvent.ModifierFlags = []
        for modifier in modifiers {
            switch modifier.lowercased() {
            case "shift":
                modifierFlags.insert(.shift)
            case "control", "ctrl":
                modifierFlags.insert(.control)
            case "option", "alt":
                modifierFlags.insert(.option)
            case "command", "cmd", "meta":
                modifierFlags.insert(.command)
            default:
                break
            }
        }

        guard let keyCode = keyNameToCode(key) else { return false }

        let timestamp = ProcessInfo.processInfo.systemUptime

        // Key down
        if let keyDown = NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: modifierFlags,
            timestamp: timestamp,
            windowNumber: window.windowNumber,
            context: nil,
            characters: key,
            charactersIgnoringModifiers: key.lowercased(),
            isARepeat: false,
            keyCode: keyCode
        ) {
            window.sendEvent(keyDown)
        }

        // Key up
        if let keyUp = NSEvent.keyEvent(
            with: .keyUp,
            location: .zero,
            modifierFlags: modifierFlags,
            timestamp: timestamp + 0.05,
            windowNumber: window.windowNumber,
            context: nil,
            characters: key,
            charactersIgnoringModifiers: key.lowercased(),
            isARepeat: false,
            keyCode: keyCode
        ) {
            window.sendEvent(keyUp)
        }

        return true
    }

    // MARK: - Focus

    /// Focus a view (make it first responder)
    static func focus(view: NSView) -> Bool {
        guard let window = view.window else { return false }
        return window.makeFirstResponder(view)
    }

    // MARK: - Scroll

    /// Scroll a view
    static func scroll(view: NSView, deltaX: Double, deltaY: Double) -> Bool {
        guard let window = view.window else { return false }

        let boundsCenter = NSPoint(x: view.bounds.midX, y: view.bounds.midY)
        let windowPoint = view.convert(boundsCenter, to: nil)
        let timestamp = ProcessInfo.processInfo.systemUptime

        if let scrollEvent = NSEvent.scrollEvent(
            with: .scrollWheel,
            location: windowPoint,
            modifierFlags: [],
            timestamp: timestamp,
            windowNumber: window.windowNumber,
            deltaX: deltaX,
            deltaY: deltaY,
            deltaZ: 0
        ) {
            view.scrollWheel(with: scrollEvent)
            return true
        }

        return false
    }

    // MARK: - Helpers

    private static func keyNameToCode(_ name: String) -> UInt16? {
        let keyMap: [String: UInt16] = [
            "a": 0x00, "s": 0x01, "d": 0x02, "f": 0x03,
            "h": 0x04, "g": 0x05, "z": 0x06, "x": 0x07,
            "c": 0x08, "v": 0x09, "b": 0x0B, "q": 0x0C,
            "w": 0x0D, "e": 0x0E, "r": 0x0F, "y": 0x10,
            "t": 0x11, "1": 0x12, "2": 0x13, "3": 0x14,
            "4": 0x15, "6": 0x16, "5": 0x17, "9": 0x19,
            "7": 0x1A, "8": 0x1C, "0": 0x1D, "o": 0x1F,
            "u": 0x20, "i": 0x22, "p": 0x23, "l": 0x25,
            "j": 0x26, "k": 0x28, "n": 0x2D, "m": 0x2E,
            "return": 0x24, "enter": 0x24,
            "tab": 0x30, "space": 0x31,
            "delete": 0x33, "backspace": 0x33,
            "escape": 0x35, "esc": 0x35,
            "left": 0x7B, "right": 0x7C,
            "down": 0x7D, "up": 0x7E,
        ]

        return keyMap[name.lowercased()]
    }
}

// MARK: - NSEvent Extension

private extension NSEvent {
    static func scrollEvent(
        with type: NSEvent.EventType,
        location: NSPoint,
        modifierFlags: NSEvent.ModifierFlags,
        timestamp: TimeInterval,
        windowNumber: Int,
        deltaX: Double,
        deltaY: Double,
        deltaZ: Double
    ) -> NSEvent? {
        // Use CGEvent for scroll events since NSEvent doesn't have a direct initializer
        guard let cgEvent = CGEvent(
            scrollWheelEvent2Source: nil,
            units: .pixel,
            wheelCount: 2,
            wheel1: Int32(deltaY),
            wheel2: Int32(deltaX),
            wheel3: 0
        ) else { return nil }

        cgEvent.location = location
        return NSEvent(cgEvent: cgEvent)
    }
}
