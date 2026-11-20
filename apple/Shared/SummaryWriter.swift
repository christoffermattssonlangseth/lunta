import Foundation
import CryptoKit
import FoundationModels

/// How the reader's Summary card is written (the prompt, the budget, checking the answer). Only
/// Foundation and FoundationModels, so it can be tried outside the app on sample papers.
enum SummaryWriter {
    /// Words of the paper given to the model (luntaSummarySource's budget). Apple's on-device model
    /// holds 4,096 tokens, prompt and answer together; the prompt is measured and cut further if needed.
    static let wordBudget = 1900
    /// Tokens kept free for the answer (3–4 sentences are about 120–180).
    static let answerTokens = 300

    static let instructions = """
        You summarize scientific papers for researchers. You get excerpts from one paper: its abstract, \
        the aims from the end of its introduction, and the start of its discussion and conclusion. \
        Write 3 or 4 plain sentences (60 to 110 words) on what the study did and what it found: \
        the question, the approach, and the main findings, with key numbers when the excerpts give them. \
        Use only the excerpts and the paper's own terms. Leave out background, praise and hype. \
        Start with the substance: never begin with "This paper", "This study", "The study", "The authors" or "In this". \
        No title, no list, no headings, no preamble.
        """

    static func prompt(title: String, source: String) -> String {
        let t = title.trimmingCharacters(in: .whitespacesAndNewlines)
        return (t.isEmpty ? "" : "Title: \(t)\n\n") + source + "\n\nSummarize this paper in 3 or 4 sentences."
    }

    /// The key a saved summary is kept under: a different reading view (remade, or from another
    /// source) gives a different text, so its summary is made again.
    static func sourceHash(_ source: String) -> String {
        SHA256.hash(data: Data(source.utf8)).prefix(12).map { String(format: "%02x", $0) }.joined()
    }

    /// The model's answer as one plain paragraph: no thinking trace, markdown, label or quotes.
    static func clean(_ raw: String) -> String {
        var s = raw
        if s.contains("<think>") {
            guard let end = s.range(of: "</think>") else { return "" }
            s = String(s[end.upperBound...])
        }
        s = s.replacingOccurrences(of: "**", with: "").replacingOccurrences(of: "__", with: "")
        s = s.replacingOccurrences(of: #"^\s*(#+\s*)?(Summary|TL;DR)\s*[:\-–—]\s*"#, with: "", options: [.regularExpression, .caseInsensitive])
        s = s.replacingOccurrences(of: #"(?m)^\s*([-*•]|\d+[.)])\s+"#, with: "", options: .regularExpression)
        s = s.components(separatedBy: .whitespacesAndNewlines).filter { !$0.isEmpty }.joined(separator: " ")
        s = s.trimmingCharacters(in: CharacterSet(charactersIn: "\"“”'"))
        return s.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// A real summary, not a refusal or a fragment.
    static func acceptable(_ s: String) -> Bool {
        guard s.count >= 80, s.split(separator: " ").count >= 15 else { return false }
        let low = s.lowercased()
        let refusals = ["i'm sorry", "i am sorry", "i cannot", "i can't", "i can not", "as an ai", "i'm unable", "i am unable",
                        "cannot assist", "can't assist", "not able to help", "i apologize"]
        return !refusals.contains { low.hasPrefix($0) || low.contains(" \($0)") }
    }

    /// The source cut until the prompt fits: `source(words)` gives the page's text within so many
    /// words, `tokens(prompt)` measures it (nil: no measuring, the word budget stands).
    static func fit(limit: Int, source: (Int) async -> String?, tokens: (String) async -> Int?) async -> String? {
        var words = wordBudget
        guard var text = await source(words) else { return nil }
        for _ in 0..<4 {
            guard let n = await tokens(text), n > limit else { return text }
            words = max(250, Int(Double(words) * Double(limit) / Double(n) * 0.92))
            guard let t = await source(words) else { return nil }
            text = t
            if words == 250 { return text }
        }
        return text
    }

    /// Writes the summary, calling `partial` with the text so far (already cleaned) as it comes.
    /// Throws what the model throws (guardrails, refusals, context); returns nil for an answer that
    /// isn't a summary.
    @available(macOS 26.0, iOS 26.0, *)
    static func write(_ session: LanguageModelSession, prompt: String,
                      partial: @MainActor (String) -> Void) async throws -> String? {
        var last = ""
        let options = GenerationOptions(temperature: 0.2, maximumResponseTokens: answerTokens)
        for try await snap in session.streamResponse(to: prompt, options: options) {
            try Task.checkCancellation()
            let t = clean(snap.content)
            if t != last, !t.isEmpty { last = t; await partial(t) }
        }
        try Task.checkCancellation()
        return acceptable(last) ? last : nil
    }

    /// Apple's on-device model, with the guardrails meant for working on text it is given (papers
    /// on drugs, suicide or violence are otherwise often refused).
    @available(macOS 26.0, iOS 26.0, *)
    static var appleModel: SystemLanguageModel {
        SystemLanguageModel(useCase: .general, guardrails: .permissiveContentTransformations)
    }

    /// Tokens a prompt takes in Apple's model, with the instructions (nil before macOS/iOS 26.4).
    static func appleTokens(_ prompt: String) async -> Int? {
        guard #available(macOS 26.4, iOS 26.4, *) else { return nil }
        let m = SystemLanguageModel.default
        guard let p = try? await m.tokenCount(for: prompt), let i = try? await m.tokenCount(for: Instructions(instructions)) else { return nil }
        return p + i
    }
}
