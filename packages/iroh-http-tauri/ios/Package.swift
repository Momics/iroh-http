// swift-tools-version:5.3

import PackageDescription

let package = Package(
    name: "tauri-plugin-iroh-http",
    platforms: [
        .macOS(.v10_13),
        .iOS(.v14),
    ],
    products: [
        .library(
            name: "tauri-plugin-iroh-http",
            type: .static,
            targets: ["tauri-plugin-iroh-http"]),
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api")
    ],
    targets: [
        .target(
            name: "tauri-plugin-iroh-http",
            dependencies: [
                .byName(name: "Tauri")
            ],
            path: "Sources")
    ]
)
