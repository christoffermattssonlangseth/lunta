import SwiftUI

// SwiftUI modifiers new in macOS/iOS 26 with what they fall back to on macOS 15 / iOS 18, where
// Lunta still runs (the deployment targets). Each is a no-op or the older equivalent there.

extension View {
    /// A solid edge under the toolbar (macOS/iOS 26: content otherwise slides under the floating
    /// toolbar items); before 26 the toolbar has its own opaque background, so nothing to do.
    @ViewBuilder func hardTopScrollEdge() -> some View {
        if #available(macOS 26.0, iOS 26.0, *) {
            scrollEdgeEffectStyle(.hard, for: .top)
        } else {
            self
        }
    }
}
