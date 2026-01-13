// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "AppDebugKit",
    platforms: [
        .macOS(.v12)
    ],
    products: [
        .library(
            name: "AppDebugKit",
            targets: ["AppDebugKit"]
        ),
    ],
    dependencies: [],
    targets: [
        .target(
            name: "AppDebugKit",
            dependencies: []
        ),
        .testTarget(
            name: "AppDebugKitTests",
            dependencies: ["AppDebugKit"]
        ),
    ]
)
