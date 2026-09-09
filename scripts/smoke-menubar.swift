// macOS native menu-bar test: swift scripts/smoke-menubar.swift <pid> [--quit]
// Requires Accessibility. Exercises the current queue, briefly plays/skips,
// leaves playback paused, and optionally quits the app. No account writes.
import Cocoa
import ApplicationServices
import CoreGraphics
import Darwin
func fail(_ text: String) -> Never { fputs("FAIL: \(text)\n", stderr); exit(1) }
guard CommandLine.arguments.count >= 2, let pid = pid_t(CommandLine.arguments[1]) else { fail("provide PID") }
let app = AXUIElementCreateApplication(pid)
AXUIElementSetMessagingTimeout(app, 0.3)
func value(_ e: AXUIElement, _ key: String) -> CFTypeRef? { var v: CFTypeRef?; AXUIElementCopyAttributeValue(e,key as CFString,&v); return v }
func children(_ e: AXUIElement) -> [AXUIElement] { value(e,"AXChildren") as? [AXUIElement] ?? [] }
func item() -> AXUIElement {
 guard let extras=value(app,"AXExtrasMenuBar"), let first=children(extras as! AXUIElement).first else { fail("missing menu-bar item") }; return first
}
func items() -> [AXUIElement] { guard let menu=children(item()).first else { return [] }; return children(menu) }
func titles() -> [String] { items().compactMap { value($0,"AXTitle") as? String } }
func settle() { RunLoop.current.run(until:Date(timeIntervalSinceNow:0.15)) }
func wait(_ text: String, _ condition: () -> Bool) {
 let end=Date(timeIntervalSinceNow:5)
 repeat { if condition() { print("PASS: \(text)"); return }; settle() } while Date() < end
 fail(text)
}
func press(_ title: String) {
 // Opening a native tracking menu may time out while the menu remains open.
 _ = AXUIElementPerformAction(item(),kAXPressAction as CFString)
 settle()
 guard let row=items().first(where:{value($0,"AXTitle") as? String == title}) else { fail("missing \(title)") }
 guard AXUIElementPerformAction(row,kAXPressAction as CFString) == .success else { fail("cannot press \(title)") }
 settle()
}
func windows() -> [[String:Any]] {
 let all=CGWindowListCopyWindowInfo([.optionOnScreenOnly,.excludeDesktopElements],kCGNullWindowID) as? [[String:Any]] ?? []
 return all.filter { ($0[kCGWindowOwnerPID as String] as? Int32)==pid && ($0[kCGWindowLayer as String] as? Int)==0 }
}
if titles().contains("Show ytamp") { press("Show ytamp") }
if titles().contains("Pause") { press("Pause") }
press("Play"); wait("Play reaches player") { titles().contains("Pause") }
press("Pause"); wait("Pause reaches player") { titles().contains("Play") }
press("Hide ytamp"); wait("Hide leaves accessible menu") { titles().contains("Show ytamp") && windows().isEmpty }
let old=titles().first
press("Next"); wait("Next advances while all windows are hidden") { titles().first != old }
press("Pause"); wait("Pause works while hidden") { titles().contains("Play") && windows().isEmpty }
press("Show ytamp"); wait("Show restores a player window") { !windows().isEmpty && titles().contains("Hide ytamp") }
press("Previous"); wait("Previous reaches player") { titles().contains("Pause") }
press("Pause")
press("Lyrics"); wait("Lyrics opens from menu") { windows().contains { ($0[kCGWindowName as String] as? String)?.hasPrefix("Lyrics") == true } }
press("Lyrics"); wait("Lyrics closes from menu") { !windows().contains { ($0[kCGWindowName as String] as? String)?.hasPrefix("Lyrics") == true } }
if CommandLine.arguments.contains("--quit") {
 press("Quit ytamp"); wait("Quit exits cleanly") { kill(pid,0) != 0 }
}
