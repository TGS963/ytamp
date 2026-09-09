// macOS native viewport smoke test. Run from the main window of a freshly
// launched ytamp: swift scripts/smoke-winamp.swift <ytamp-pid>
// Requires Accessibility permission for the terminal. Does not start audio,
// change EQ gains, or write account data. Leaves the main window visible.
import Cocoa
import ApplicationServices
import CoreGraphics

func fail(_ message: String) -> Never {
    fputs("FAIL: \(message)\n", stderr)
    exit(1)
}
guard CommandLine.arguments.count == 2,
      let pid = pid_t(CommandLine.arguments[1]) else { fail("provide the ytamp PID") }
guard AXIsProcessTrusted() else { fail("enable Accessibility for this terminal") }
let app = AXUIElementCreateApplication(pid)
func settle() { RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.35)) }
func front() {
    guard AXUIElementSetAttributeValue(app, kAXFrontmostAttribute as CFString, kCFBooleanTrue) == .success else { fail("cannot focus ytamp") }
    settle()
}
func window() -> CGRect {
    let deadline = Date(timeIntervalSinceNow: 3)
    repeat {
        let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
        if let info = windows.first(where: { ($0[kCGWindowOwnerPID as String] as? Int32) == pid && ($0[kCGWindowLayer as String] as? Int) == 0 }),
           let bounds = info[kCGWindowBounds as String] as? [String: Any],
           let rect = CGRect(dictionaryRepresentation: bounds as CFDictionary) { return rect }
        settle()
    } while Date() < deadline
    fail("no visible ytamp window; the process may have crashed")
}
func toggle() {
    front()
    for down in [true, false] {
        let event = CGEvent(keyboardEventSource: nil, virtualKey: 46, keyDown: down)!
        event.flags = [.maskCommand, .maskShift]
        event.post(tap: .cghidEventTap)
        Thread.sleep(forTimeInterval: 0.08)
    }
    settle()
}
func click(_ x: CGFloat, _ y: CGFloat) {
    front()
    let rect = window()
    let unit = rect.width / 275
    let point = CGPoint(x: rect.minX + x * unit, y: rect.minY + y * unit)
    for kind in [CGEventType.mouseMoved, .leftMouseDown, .leftMouseUp] {
        CGEvent(mouseEventSource: nil, mouseType: kind, mouseCursorPosition: point, mouseButton: .left)?.post(tap: .cghidEventTap)
        Thread.sleep(forTimeInterval: 0.08)
    }
    settle()
}
func waitWindow(_ description: String, _ matches: (CGRect) -> Bool) -> CGRect {
    let deadline = Date(timeIntervalSinceNow: 5)
    repeat {
        let rect = window()
        if matches(rect) { return rect }
        settle()
    } while Date() < deadline
    fail(description)
}
func height(_ expected: CGFloat) {
    _ = waitWindow("expected skin height \(expected)") { rect in
        abs(rect.height / (rect.width / 275) - expected) < 2
    }
}
front()
let main = window()
for cycle in 1...3 {
    toggle()
    height(116)
    click(230, 64) // EQ
    height(232)
    click(252, 64) // PL
    let full = waitWindow("playlist did not open") { $0.height / ($0.width / 275) > 232 }
    let fullHeight = full.height / (full.width / 275)
    click(259, 123) // EQ shade
    height(fullHeight - 102)
    click(259, 123) // restore EQ; next cycle starts expanded
    height(fullHeight)
    click(268, 123) // close EQ without changing its enabled state
    height(fullHeight - 116)
    click(252, 64) // close PL
    height(116)
    toggle()
    _ = waitWindow("main window did not return") {
        abs($0.width - main.width) < 2 && abs($0.height - main.height) < 2
    }
    print("PASS: native player / EQ / playlist / shade / return cycle \(cycle)")
}
