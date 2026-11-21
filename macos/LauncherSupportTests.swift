import Foundation

@main
struct LauncherSupportTests {
    static func main() throws {
        let fm = FileManager.default
        let root = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
        let resources = root.appendingPathComponent("resources")
        let support = root.appendingPathComponent("support")
        let legacy = support.appendingPathComponent(LauncherSupport.legacyDirectoryName)
        try fm.createDirectory(at: legacy.appendingPathComponent("saves"), withIntermediateDirectories: true)
        try Data("synthetic-save".utf8).write(to: legacy.appendingPathComponent("saves/test.txt"))
        let base = try LauncherSupport.prepareDataDirectory(in: support)
        assert(try! Data(contentsOf: base.appendingPathComponent("saves/test.txt")) == Data("synthetic-save".utf8))
        assert(fm.fileExists(atPath: legacy.appendingPathComponent("saves/test.txt").path))
        for path in LauncherSupport.managedResources {
            let file = resources.appendingPathComponent(path)
            try fm.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
            try Data("version-one".utf8).write(to: file)
        }
        try LauncherSupport.refreshResources(from: resources, to: base)
        let table = base.appendingPathComponent("zh-tw.tsv")
        let oldDate = Date(timeIntervalSince1970: 100)
        try fm.setAttributes([.modificationDate: oldDate], ofItemAtPath: table.path)
        try LauncherSupport.refreshResources(from: resources, to: base)
        let modificationDate = try fm.attributesOfItem(atPath: table.path)[.modificationDate] as! Date
        assert(modificationDate == oldDate)
        try Data("version-two".utf8).write(to: resources.appendingPathComponent("zh-tw.tsv"))
        try Data("font-two".utf8).write(to: resources.appendingPathComponent("fonts/LXGWWenKaiTC-Regular.ttf"))
        try LauncherSupport.refreshResources(from: resources, to: base)
        assert(try! Data(contentsOf: table) == Data("version-two".utf8))
        let backups = try fm.contentsOfDirectory(at: base.appendingPathComponent("translation-backups"), includingPropertiesForKeys: nil)
        assert(backups.count == 1)
        assert(try! Data(contentsOf: backups[0].appendingPathComponent("zh-tw.tsv")) == Data("version-one".utf8))
        assert(try! Data(contentsOf: backups[0].appendingPathComponent("fonts/LXGWWenKaiTC-Regular.ttf")) == Data("version-one".utf8))
        // Existing stable data wins; repeating migration never replaces new saves.
        try Data("new-save".utf8).write(to: base.appendingPathComponent("saves/test.txt"))
        _ = try LauncherSupport.prepareDataDirectory(in: support)
        assert(try! Data(contentsOf: base.appendingPathComponent("saves/test.txt")) == Data("new-save".utf8))
        print("PASS: migration preserves original; stable data wins; unchanged resources untouched; tables/fonts refreshed and backed up; saves preserved")
    }
}
