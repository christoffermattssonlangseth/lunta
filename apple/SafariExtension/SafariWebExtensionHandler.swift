import Foundation
import SafariServices

/// The native side of Lunta's Safari extension (the extension itself is extension/, the same files
/// as in Chrome and Firefox).
///
/// - Mac: unused. background.js talks to Lunta for Mac on 127.0.0.1:47480, as in Chrome.
/// - iPhone and iPad: background.js sends its requests here as native messages (there is no Lunta
///   server on the device). This only writes them into the App Group for the app (SafariInbox),
///   in pieces, and answers "status" from the index the app keeps there: no conversion here, an
///   extension has little memory. The app does the work when it is next opened.
final class SafariWebExtensionHandler: NSObject, NSExtensionRequestHandling {
    func beginRequest(with context: NSExtensionContext) {
        let item = context.inputItems.first as? NSExtensionItem
        let message = item?.userInfo?[SFExtensionMessageKey] as? [String: Any] ?? [:]
        #if os(iOS)
        let reply: [String: Any]
        if let root = SafariInbox.groupRoot {
            reply = SafariInbox.handle(message, root: root)
        } else {
            reply = ["error": "Lunta's shared folder isn't available. Open the Lunta app once, then try again."]
        }
        #else
        let reply: [String: Any] = message["op"] as? String == "ping" ? ["platform": "macos"] : ["error": "Lunta for Mac answers on 127.0.0.1:47480."]
        #endif
        let response = NSExtensionItem()
        response.userInfo = [SFExtensionMessageKey: reply]
        context.completeRequest(returningItems: [response], completionHandler: nil)
    }
}
