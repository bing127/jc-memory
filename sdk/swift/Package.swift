// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "JiangchengMemory",
    platforms: [
        .macOS(.v13),
        .iOS(.v16),
    ],
    products: [
        .library(name: "JiangchengMemory", targets: ["JiangchengMemory"]),
        .library(name: "JiangchengMemoryEmbedded", targets: ["JiangchengMemoryEmbedded"]),
    ],
    targets: [
        .target(name: "JiangchengMemory"),
        .target(
            name: "jc_memory_uniffiFFI",
            path: "Sources/jc_memory_uniffiFFI",
            publicHeadersPath: "."
        ),
        .target(
            name: "JiangchengMemoryUniffi",
            dependencies: ["jc_memory_uniffiFFI"],
            path: "Sources/JiangchengMemoryUniffi"
        ),
        .target(
            name: "JiangchengMemoryEmbedded",
            dependencies: ["JiangchengMemoryUniffi"],
            path: "Sources/JiangchengMemoryEmbedded"
        ),
        .testTarget(name: "JiangchengMemoryTests", dependencies: ["JiangchengMemory"]),
    ]
)
