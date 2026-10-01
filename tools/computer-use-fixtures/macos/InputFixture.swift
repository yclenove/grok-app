// Owned AppKit postcondition fixture, never a production input implementation.
// The private pipe can query, move, focus, or quit this process's one window.
// It deliberately has NO text/key/click injection or expectation-setting command.
import AppKit
import Foundation
import Darwin

// Check THIS process, not the parent shell or compiler process. A translated
// fixture must not be counted as the other architecture's native Cocoa proof.
func nativeProcess() -> Bool {
    var value: Int32 = 0
    var size = MemoryLayout<Int32>.size
    let status = sysctlbyname("sysctl.proc_translated", &value, &size, nil, 0)
    if status == -1 {
        return errno == ENOENT
    }
    return status == 0 && size == MemoryLayout<Int32>.size && value == 0
}
#if arch(arm64)
let fixtureArchitecture = "aarch64"
#elseif arch(x86_64)
let fixtureArchitecture = "x86_64"
#else
let fixtureArchitecture = "unsupported"
#endif

final class InputView: NSTextView {
    var keys: [[String: Any]] = []
    override func keyDown(with event: NSEvent) {
        keys.append(["code": Int(event.keyCode), "down": true])
        super.keyDown(with: event)
    }
    override func keyUp(with event: NSEvent) {
        keys.append(["code": Int(event.keyCode), "down": false])
        super.keyUp(with: event)
    }
}

// An actual interactive view: wheel input moves its rendered ruler and a left
// drag moves the blue box. Pipe commands cannot set either postcondition.
final class PointerView: NSView {
    var events: [[String: Any]] = []
    var overflow = false
    var scrollOffset: CGFloat = 500
    var box = NSRect(x: 290, y: 70, width: 80, height: 40)
    var dragOffset: NSPoint?
    override var isFlipped: Bool { true }

    func record(_ event: NSEvent, kind: String) {
        guard events.count < 96 else { overflow = true; return }
        let point = convert(event.locationInWindow, from: nil)
        let wheel = kind == "wheel"
        // AppKit documents clickCount as meaningful only for down/up. A slow
        // drag's up may report 0; do not manufacture click counts for motion.
        let count = (kind == "down" || kind == "up") ? event.clickCount : 0
        events.append([
            "kind": kind, "button": wheel ? 0 : event.buttonNumber,
            "clickCount": count,
            "x": Double(point.x), "y": Double(point.y),
            "deltaY": wheel ? Double(event.scrollingDeltaY) : 0,
            "flags": event.modifierFlags.intersection(.deviceIndependentFlagsMask).rawValue
        ])
    }
    override func mouseDown(with event: NSEvent) {
        record(event, kind: "down")
        let point = convert(event.locationInWindow, from: nil)
        if box.contains(point) {
            dragOffset = NSPoint(x: point.x - box.minX, y: point.y - box.minY)
        }
    }
    override func mouseDragged(with event: NSEvent) {
        record(event, kind: "drag")
        if let offset = dragOffset {
            let point = convert(event.locationInWindow, from: nil)
            box.origin = NSPoint(x: min(max(0, point.x - offset.x), bounds.width - box.width),
                                 y: min(max(0, point.y - offset.y), bounds.height - box.height))
            needsDisplay = true
        }
    }
    override func mouseUp(with event: NSEvent) {
        record(event, kind: "up")
        dragOffset = nil
    }
    override func rightMouseDown(with event: NSEvent) { record(event, kind: "down") }
    override func rightMouseUp(with event: NSEvent) { record(event, kind: "up") }
    override func otherMouseDown(with event: NSEvent) { record(event, kind: "down") }
    override func otherMouseUp(with event: NSEvent) { record(event, kind: "up") }
    override func scrollWheel(with event: NSEvent) {
        record(event, kind: "wheel")
        scrollOffset = min(max(0, scrollOffset - event.scrollingDeltaY), 1000)
        needsDisplay = true
    }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.controlBackgroundColor.setFill()
        NSBezierPath(rect: bounds).fill()
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedSystemFont(ofSize: 12, weight: .regular),
            .foregroundColor: NSColor.labelColor
        ]
        for row in 0..<60 {
            let y = CGFloat(row * 24) - scrollOffset
            if y >= 0 && y < bounds.height {
                NSString(string: "Owned ruler \(row)").draw(at: NSPoint(x: 8, y: y), withAttributes: attributes)
            }
        }
        NSColor.systemBlue.setFill()
        NSBezierPath(rect: box).fill()
    }
    func state() -> [String: Any] {
        ["width": Double(bounds.width), "height": Double(bounds.height),
         "scrollOffset": Double(scrollOffset),
         "boxRect": [Double(box.minX), Double(box.minY), Double(box.width), Double(box.height)],
         "dragging": dragOffset != nil, "overflow": overflow, "events": events]
    }
}

final class Fixture: NSObject, NSApplicationDelegate {
    let nonce: String
    let text = InputView(frame: NSRect(x: 30, y: 330, width: 660, height: 220))
    let pointer = PointerView(frame: NSRect(x: 30, y: 100, width: 660, height: 180))
    let window: NSWindow
    var sequence = 0
    var clicks = 0
    var stopping = false

    init(nonce: String) {
        self.nonce = nonce
        window = NSWindow(contentRect: NSRect(x: 120, y: 140, width: 720, height: 600),
                          styleMask: [.titled, .closable], backing: .buffered, defer: false)
        super.init()
        window.title = "GrokCuOwned-\(nonce)"
        window.isReleasedWhenClosed = false
        text.isRichText = false
        text.isAutomaticQuoteSubstitutionEnabled = false
        text.isAutomaticDashSubstitutionEnabled = false
        text.isAutomaticTextReplacementEnabled = false
        text.isAutomaticSpellingCorrectionEnabled = false
        text.font = NSFont.monospacedSystemFont(ofSize: 18, weight: .regular)
        text.string = "原有😀text"
        text.setAccessibilityLabel("Owned input \(nonce)")
        text.setAccessibilityIdentifier("owned-text")
        window.contentView?.addSubview(text)
        pointer.setAccessibilityElement(true)
        pointer.setAccessibilityRole(.group)
        pointer.setAccessibilityLabel("Owned pointer \(nonce)")
        pointer.setAccessibilityIdentifier("owned-pointer")
        window.contentView?.addSubview(pointer)
        let button = NSButton(title: "Owned press \(nonce)", target: self,
                              action: #selector(pressed(_:)))
        button.frame = NSRect(x: 30, y: 25, width: 260, height: 35)
        button.setAccessibilityIdentifier("owned-press")
        window.contentView?.addSubview(button)
        let secret = NSSecureTextField(frame: NSRect(x: 320, y: 25, width: 240, height: 35))
        secret.stringValue = "fixture-only-never-observe-\(nonce)"
        secret.setAccessibilityLabel("Owned protected \(nonce)")
        window.contentView?.addSubview(secret)
    }

    @objc func pressed(_ sender: NSButton) { clicks += 1 }

    func activateOwnedWindow() {
        window.makeKeyAndOrderFront(nil)
        if #available(macOS 14.0, *) {
            NSApp.activate()
        } else {
            NSApp.activate(ignoringOtherApps: true)
        }
        _ = window.makeFirstResponder(text)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        activateOwnedWindow()
        text.setSelectedRange(NSRange(location: 0, length: 0))
        reply(id: 0, error: nil)
        // Pipe EOF and the watchdog retire only our fixture, not the user's App.
        DispatchQueue.global().async { [self] in
            while let line = readLine() {
                if line.utf8.count > 4096 { break }
                DispatchQueue.main.sync { self.command(line) }
            }
            DispatchQueue.main.async { NSApp.terminate(nil) }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 90) { NSApp.terminate(nil) }
    }

    func command(_ line: String) {
        guard !stopping,
              let data = line.data(using: .utf8),
              let request = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              request.count == 4, request["version"] as? Int == 2,
              request["nonce"] as? String == nonce,
              let id = request["id"] as? Int, id == sequence + 1,
              let command = request["command"] as? String else {
            stopping = true
            reply(id: -1, error: "invalid fixture request")
            NSApp.terminate(nil)
            return
        }
        sequence = id
        switch command {
        case "state": break
        case "focus":
            activateOwnedWindow()
        case "move":
            window.setFrameOrigin(NSPoint(x: window.frame.minX + 24, y: window.frame.minY + 18))
        case "quit": stopping = true
        default:
            stopping = true
            reply(id: id, error: "unsupported fixture request")
            NSApp.terminate(nil)
            return
        }
        reply(id: id, error: nil)
        if stopping { NSApp.terminate(nil) }
    }

    func reply(id: Int, error: String?) {
        let selection = text.selectedRange()
        var state: [String: Any] = [
            "version": 2, "nonce": nonce, "id": id,
            "pid": Int(ProcessInfo.processInfo.processIdentifier),
            "architecture": fixtureArchitecture, "translated": false,
            "windowId": window.windowNumber, "title": window.title,
            "active": NSApp.isActive, "focused": window.isKeyWindow && window.firstResponder === text,
            "text": text.string, "selection": [selection.location, selection.length],
            "keys": Array(text.keys.suffix(64)), "clicks": clicks,
            "pointer": pointer.state()
        ]
        if let error = error { state["error"] = error }
        guard let data = try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]) else {
            NSApp.terminate(nil)
            return
        }
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([10]))
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

guard CommandLine.arguments.count == 3, CommandLine.arguments[1] == "--owned-fixture",
      UUID(uuidString: CommandLine.arguments[2]) != nil,
      ProcessInfo.processInfo.environment["GROK_CU_MACOS_FIXTURE"] == "owned-cocoa" else {
    FileHandle.standardError.write(Data("not_run: explicit owned fixture opt-in required\n".utf8))
    exit(2)
}
guard fixtureArchitecture != "unsupported", nativeProcess() else {
    FileHandle.standardError.write(Data("not_run: translated or unknown fixture architecture\n".utf8))
    exit(2)
}
let application = NSApplication.shared
application.setActivationPolicy(.regular)
let fixture = Fixture(nonce: CommandLine.arguments[2])
application.delegate = fixture
application.run()
