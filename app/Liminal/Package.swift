// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Liminal",
    platforms: [.macOS(.v14)],
    targets: [
        .target(
            name: "LiminalCore",
            path: "Sources/LiminalCore",
        ),
        .executableTarget(
            name: "liminal-doctor",
            dependencies: ["LiminalCore"],
            path: "Sources/liminal-doctor",
        ),
        .testTarget(
            name: "LiminalCoreTests",
            dependencies: ["LiminalCore"],
            path: "Tests/LiminalCoreTests",
        ),
    ],
)
