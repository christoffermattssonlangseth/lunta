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
    /// The work type (Crossref/CSL: "journal-article", "book", "dissertation"…), when known: one of
    /// the signals that decide whether a PDF is converted as a paper or as a book.
    public var workType = ""
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
        if workType.isEmpty { workType = other.workType }
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
    /// What the converter took a PDF for (paper or book), and the signals that decided it.
    public var kind: DocumentKind = .paper
    public var kindReasons: [String] = []
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
    /// Text of the reading view as Lunta's reader shows it (for checking that highlights still
    /// anchor before replacing a reading view: see `HighlightLocator`).
    public var readerText: String = ""
    /// Paper or book (a thesis is converted as a book): a library item converted as a book that has
    /// no work type yet could be given the type "book" (or "thesis").
    public var kind: DocumentKind = .paper
    /// The signals that decided `kind` ("541 pages", "outline of 24 chapters", "ISBN"…).
    public var kindReasons: [String] = []
}

public enum ConversionError: Error, LocalizedError {
    case message(String)
    public var errorDescription: String? {
        switch self { case .message(let m): return m }
    }
}

func fail(_ m: String) -> ConversionError { .message(m) }
