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
    ],
    targets: [
        .target(name: "JiangchengMemory"),
        .testTarget(name: "JiangchengMemoryTests", dependencies: ["JiangchengMemory"]),
    ]
)
