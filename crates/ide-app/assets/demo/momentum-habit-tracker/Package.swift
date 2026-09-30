// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MomentumHabitTracker",
    platforms: [.iOS(.v18), .macOS(.v13)],
    products: [.library(name: "MomentumApp", targets: ["MomentumApp"])],
    targets: [
        .target(name: "MomentumApp"),
        .testTarget(name: "MomentumAppTests", dependencies: ["MomentumApp"])
    ]
)
