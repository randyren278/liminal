// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Liminal",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "https://github.com/apple/swift-protobuf.git", from: "1.28.0"),
    ],
    targets: [
        .target(
            name: "LiminalCore",
            dependencies: [
                .product(name: "SwiftProtobuf", package: "swift-protobuf"),
            ],
            path: "Sources/LiminalCore",
        ),
        .executableTarget(
            name: "liminal-doctor",
            dependencies: ["LiminalCore"],
            path: "Sources/liminal-doctor",
        ),
        .executableTarget(
            name: "liminal-capture",
            dependencies: ["LiminalCore"],
            path: "Sources/liminal-capture",
        ),
        .testTarget(
            name: "LiminalCoreTests",
            dependencies: ["LiminalCore"],
            path: "Tests/LiminalCoreTests",
        ),
    ],
)
