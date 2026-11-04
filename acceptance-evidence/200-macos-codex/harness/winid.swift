import CoreGraphics
let pid = Int(CommandLine.arguments[1])!
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in list where (w[kCGWindowOwnerPID as String] as? Int) == pid {
    print(w[kCGWindowNumber as String]!, w[kCGWindowName as String] ?? "", w[kCGWindowBounds as String]!)
}
