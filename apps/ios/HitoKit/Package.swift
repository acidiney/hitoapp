// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "HitoKit",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [.library(name: "HitoKit", targets: ["HitoKit"])],
    targets: [
        .target(name: "HitoKit"),
        .testTarget(name: "HitoKitTests", dependencies: ["HitoKit"]),
    ]
)
