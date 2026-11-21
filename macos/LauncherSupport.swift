import Foundation

/// Only shipped translations and fonts are managed; ROMs and saves are never refreshed.
enum LauncherSupport {
    static let dataDirectoryName = "DBZ SSD"
    static let legacyDirectoryName = "DBZ SSD RC2"
    static let managedResources = [
        "zh-tw.tsv", "ui-zh-tw.tsv", "zh-hk.tsv", "ui-zh-hk.tsv",
        "fonts/LXGWWenKaiTC-Regular.ttf"
    ]

    static func prepareDataDirectory(in support: URL, fileManager fm: FileManager = .default) throws -> URL {
        let base = support.appendingPathComponent(dataDirectoryName, isDirectory: true)
        let legacy = support.appendingPathComponent(legacyDirectoryName, isDirectory: true)
        if !fm.fileExists(atPath: base.path), fm.fileExists(atPath: legacy.path) {
            // Copy first, then rename. An interrupted migration cannot become the active directory.
            let staging = support.appendingPathComponent(".DBZ-SSD-migration-" + UUID().uuidString)
            defer { try? fm.removeItem(at: staging) }
            try fm.copyItem(at: legacy, to: staging)
            try fm.moveItem(at: staging, to: base)
        }
        try fm.createDirectory(at: base, withIntermediateDirectories: true)
        return base
    }

    static func refreshResources(from resources: URL, to base: URL, fileManager fm: FileManager = .default) throws {
        let backup = base.appendingPathComponent("translation-backups/" + UUID().uuidString)
        for relativePath in managedResources {
            let source = resources.appendingPathComponent(relativePath)
            let destination = base.appendingPathComponent(relativePath)
            let bundled = try Data(contentsOf: source)
            if fm.fileExists(atPath: destination.path) {
                if try Data(contentsOf: destination) == bundled { continue }
                // Preserve user edits and old translations before replacing changed bundled content.
                let saved = backup.appendingPathComponent(relativePath)
                try fm.createDirectory(at: saved.deletingLastPathComponent(), withIntermediateDirectories: true)
                try fm.copyItem(at: destination, to: saved)
            }
            try fm.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
            try bundled.write(to: destination, options: .atomic)
        }
    }
}
