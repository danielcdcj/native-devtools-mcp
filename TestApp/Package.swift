// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "TestApp",
    platforms: [
        .macOS(.v14)
    ],
    dependencies: [
        .package(path: "../AppDebugKit")
    ],
    targets: [
        .executableTarget(
            name: "TestApp",
            dependencies: ["AppDebugKit"]
        ),
    ]
)
