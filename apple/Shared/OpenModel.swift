import Foundation
import FoundationModels
import MLXFoundationModels
import MLXLLM
import MLXLMCommon
import HuggingFace
import Tokenizers

/// An open model (Qwen3 4B, 4-bit) running locally with MLX, for devices where Apple Intelligence
/// isn't available (e.g. iPhones in the EU). It plugs into FoundationModels, so Ask's tools,
/// streaming and citations work exactly as with Apple's model. Downloaded once, then offline.
@available(iOS 27.0, macOS 27.0, *)
@MainActor
final class OpenModel: ObservableObject {
    static let shared = OpenModel()
    static let displayName = "Qwen3 4B"
    static let downloadSize = "about 2.3 GB"

    enum State: Equatable { case notDownloaded, downloading(Double), loading, ready, failed(String) }
    @Published private(set) var state: State

    let model: MLXLanguageModel

    private init() {
        model = MLXLanguageModel(
            configuration: LLMRegistry.qwen3_4b_4bit,
            capabilities: [.toolCalling, .guidedGeneration],
            weightsLocation: { id in
                let cache = HubCache.default
                guard let repo = Repo.ID(rawValue: id) else { return cache.cacheDirectory }
                if let commit = cache.resolveRevision(repo: repo, kind: .model, ref: "main"),
                   let snapshot = try? cache.snapshotPath(repo: repo, kind: .model, commitHash: commit) {
                    return snapshot
                }
                return cache.repoDirectory(repo: repo, kind: .model)
            },
            load: { configuration, progress in
                try await loadModelContainer(from: HubDownloader(HubClient()), using: TransformersTokenizerLoader(),
                                             configuration: configuration, progressHandler: { p in
                    progress(p)
                    let f = p.fractionCompleted
                    Task { @MainActor in
                        if case .downloading = OpenModel.shared.state { OpenModel.shared.state = .downloading(f) }
                    }
                })
            })
        state = UserDefaults.standard.bool(forKey: "openModelDownloaded") ? .loading : .notDownloaded
        if state == .loading { Task { await prepare() } }
    }

    var isReady: Bool { state == .ready || state == .loading }

    /// Downloads (first time) and loads the model.
    func prepare() async {
        if state == .ready { return }
        if !UserDefaults.standard.bool(forKey: "openModelDownloaded") { state = .downloading(0) }
        do {
            try await model.preload()
            UserDefaults.standard.set(true, forKey: "openModelDownloaded")
            state = .ready
        } catch {
            NSLog("Margins: open model failed: %@", String(describing: error))
            state = .failed(error.localizedDescription)
        }
    }
}

// Bridges between MLX and the Hugging Face packages (what the package's convenience macros would generate).

struct HubDownloader: MLXLMCommon.Downloader {
    let upstream: HuggingFace.HubClient
    init(_ upstream: HuggingFace.HubClient) { self.upstream = upstream }

    func download(id: String, revision: String?, matching patterns: [String], useLatest: Bool,
                  progressHandler: @Sendable @escaping (Foundation.Progress) -> Void) async throws -> URL {
        guard let repo = HuggingFace.Repo.ID(rawValue: id) else { throw CocoaError(.fileNoSuchFile) }
        return try await upstream.downloadSnapshot(of: repo, revision: revision ?? "main", matching: patterns,
                                                   progressHandler: { @MainActor p in progressHandler(p) })
    }
}

struct TransformersTokenizerLoader: MLXLMCommon.TokenizerLoader {
    func load(from directory: URL) async throws -> any MLXLMCommon.Tokenizer {
        TokenizerBridge(try await Tokenizers.AutoTokenizer.from(modelFolder: directory))
    }
}

struct TokenizerBridge: MLXLMCommon.Tokenizer {
    let upstream: any Tokenizers.Tokenizer
    init(_ upstream: any Tokenizers.Tokenizer) { self.upstream = upstream }

    func encode(text: String, addSpecialTokens: Bool) -> [Int] { upstream.encode(text: text, addSpecialTokens: addSpecialTokens) }
    func decode(tokenIds: [Int], skipSpecialTokens: Bool) -> String { upstream.decode(tokens: tokenIds, skipSpecialTokens: skipSpecialTokens) }
    func convertTokenToId(_ token: String) -> Int? { upstream.convertTokenToId(token) }
    func convertIdToToken(_ id: Int) -> String? { upstream.convertIdToToken(id) }
    var bosToken: String? { upstream.bosToken }
    var eosToken: String? { upstream.eosToken }
    var unknownToken: String? { upstream.unknownToken }

    func applyChatTemplate(messages: [[String: any Sendable]], tools: [[String: any Sendable]]?,
                           additionalContext: [String: any Sendable]?) throws -> [Int] {
        do {
            return try upstream.applyChatTemplate(messages: messages, tools: tools, additionalContext: additionalContext)
        } catch Tokenizers.TokenizerError.missingChatTemplate {
            throw MLXLMCommon.TokenizerError.missingChatTemplate
        }
    }
}
