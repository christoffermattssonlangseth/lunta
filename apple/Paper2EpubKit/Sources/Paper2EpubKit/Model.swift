import Foundation

public struct PaperMeta: Sendable {
    public var title = ""
    public var authors: [String] = []
    public var date = ""
    public var doi = ""
    public var journal = ""
    public var abstract = ""
    public var license = ""
    public var keywords: [String] = []
    public var url = ""
    /// Pre-rendered XHTML for affiliations, shown on the title page.
    public var affiliationsHTML = ""

    public init() {}

    /// Fills empty fields from `other`.
    mutating func merge(_ other: PaperMeta) {
        if title.isEmpty { title = other.title }
        if authors.isEmpty { authors = other.authors }
        if date.isEmpty { date = other.date }
        if doi.isEmpty { doi = other.doi }
        if journal.isEmpty { journal = other.journal }
        if abstract.isEmpty { abstract = other.abstract }
        if license.isEmpty { license = other.license }
        if keywords.isEmpty { keywords = other.keywords }
        if url.isEmpty { url = other.url }
    }
}

public struct Heading: Sendable {
    public var level: Int        // 1 = chapter, 2 = subsection
    public var id: String
    public var title: String     // plain text
}

public struct Chapter: Sendable {
    public var title: String
    public var html: String      // XHTML body content
    public var headings: [Heading]
}

public struct BookImage: Sendable {
    public var fileName: String  // e.g. "fig1.jpg"
    public var data: Data
    public var mediaType: String
}

public struct Book: Sendable {
    public var meta: PaperMeta
    public var chapters: [Chapter]
    public var images: [BookImage]
    public var sourceLabel: String
    public var notes: [String]
}

public struct ConversionResult: Sendable {
    public var file: URL
    public var title: String
    public var source: String
    public var words: Int
    public var images: Int
    public var notes: [String]
    /// Metadata of the converted paper (for reference managers such as Lunta).
    public var meta: PaperMeta = PaperMeta()
    /// Plain text of the whole paper, for full-text search.
    public var fulltext: String = ""
}

public enum ConversionError: Error, LocalizedError {
    case message(String)
    public var errorDescription: String? {
        switch self { case .message(let m): return m }
    }
}

func fail(_ m: String) -> ConversionError { .message(m) }
