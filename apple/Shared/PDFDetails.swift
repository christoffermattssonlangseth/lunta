import Foundation
import FoundationModels
import PDFKit

/// Reads a paper's title, authors and venue from the first page of its PDF with Apple's on-device model.
/// Nothing leaves the device, so this is safe for confidential manuscripts.
enum PDFDetails {
    @Generable
    struct Extracted {
        @Guide(description: "The paper's full title exactly as printed on the page, without 'Title:' labels")
        var title: String
        @Guide(description: "Every author's name in printed order, as 'First Last', without affiliations, numbers or symbols")
        var authors: [String]
        @Guide(description: "Journal or venue name if printed, otherwise empty")
        var journal: String
        @Guide(description: "Four-digit publication year if printed, otherwise empty")
        var year: String
        @Guide(description: "DOI if printed (like 10.1234/abcd), otherwise empty")
        var doi: String
    }

    struct Result {
        var title = ""
        var authors: [String] = []
        var journal = ""
        var year = ""
        var doi = ""
    }

    static var available: Bool { SystemLanguageModel.default.isAvailable }

    static func extract(fromPDF data: Data) async -> Result? {
        guard available, let doc = PDFDocument(data: data), doc.pageCount > 0 else { return nil }
        // the first page, plus the second when the first is a short cover page
        var text = doc.page(at: 0)?.string ?? ""
        if text.count < 600, doc.pageCount > 1 { text += "\n" + (doc.page(at: 1)?.string ?? "") }
        text = String(text.prefix(3500))
        guard text.count > 40 else { return nil }
        let session = LanguageModelSession(instructions: """
            You extract bibliographic details from the first page of a scientific paper or manuscript. \
            Copy text exactly as printed; never invent anything. Leave a field empty when it isn't on the page.
            """)
        guard let r = try? await session.respond(to: "First page:\n\n\(text)", generating: Extracted.self,
                                                 options: GenerationOptions(temperature: 0)) else { return nil }
        let e = r.content
        let page = norm(text)
        var out = Result()
        // keep only what is really on the page
        if norm(e.title).count > 10, page.contains(norm(e.title)) { out.title = tidy(e.title) }
        out.authors = e.authors.map(tidy).filter { a in
            let parts = a.split(separator: " ").map { norm(String($0)) }.filter { $0.count > 1 }
            return a.count > 3 && a.count < 60 && !parts.isEmpty && parts.allSatisfy { page.contains($0) }
        }
        if norm(e.journal).count > 3, page.contains(norm(e.journal)) { out.journal = tidy(e.journal) }
        if e.year.count == 4, Int(e.year) != nil, text.contains(e.year) { out.year = e.year }
        if e.doi.hasPrefix("10."), text.contains(e.doi) { out.doi = e.doi }
        return out
    }

    private static func norm(_ s: String) -> String {
        s.folding(options: [.diacriticInsensitive, .caseInsensitive], locale: nil).filter { $0.isLetter || $0.isNumber }
    }

    private static func tidy(_ s: String) -> String {
        s.replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
            .trimmingCharacters(in: CharacterSet(charactersIn: " ,;*†‡§0123456789").union(.whitespacesAndNewlines))
    }
}
