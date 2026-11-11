// swift-tools-version:5.10
import PackageDescription

let package = Package(
    name: "Paper2EpubKit",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "Paper2EpubKit", targets: ["Paper2EpubKit"]),
        .executable(name: "p2e", targets: ["p2e"]),
    ],
    targets: [
        .target(name: "Paper2EpubKit", path: "Sources/Paper2EpubKit"),
        // macOS command-line front end, used to test conversions without the simulator
        .executableTarget(name: "p2e", dependencies: ["Paper2EpubKit"], path: "Sources/p2e"),
    ]
)
