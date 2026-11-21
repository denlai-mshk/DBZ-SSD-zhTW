import AppKit
import Foundation
import UniformTypeIdentifiers

@main
struct Launcher {
    static func dialog(_ title: String, _ message: String, _ buttons: [String]) -> Int {
        NSApplication.shared.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        for button in buttons { alert.addButton(withTitle: button) }
        return alert.runModal().rawValue - NSApplication.ModalResponse.alertFirstButtonReturn.rawValue
    }

    static func pick(_ title: String, _ extensions: [String]) -> URL? {
        let panel = NSOpenPanel()
        panel.title = title
        panel.prompt = "選擇"
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = extensions.compactMap { UTType(filenameExtension: $0) }
        return panel.runModal() == .OK ? panel.url : nil
    }

    static func main() {
        let fm = FileManager.default
        NSApplication.shared.setActivationPolicy(.accessory)
        NSApplication.shared.activate(ignoringOtherApps: true)
        do {
            let support = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            let base = try LauncherSupport.prepareDataDirectory(in: support)
            guard let resources = Bundle.main.resourceURL else {
                throw NSError(domain: "DBZLauncher", code: 1,
                              userInfo: [NSLocalizedDescriptionKey: "找不到應用程式資源。"])
            }
            try LauncherSupport.refreshResources(from: resources, to: base)
            var arguments = ["--mode", "enhanced", "--hd-scale", "4"]
            if !fm.fileExists(atPath: base.appendingPathComponent("cache/rom/canonical.sfc").path) {
                let kind = dialog("七龍珠Z 超賽亞傳說", "請選擇您自行持有的遊戲檔案。日文 Rev 1 版本另需 Klepto v1.02 IPS 修補檔。原始檔案不會被修改。", ["已修補的英文版 ROM", "日文 ROM 與 IPS", "取消"])
                guard kind < 2, let rom = pick("選擇遊戲 ROM", ["sfc", "smc"]) else { return }
                arguments += ["--rom", rom.path]
                if kind == 1 {
                    guard let ips = pick("選擇 dbz102.ips 修補檔", ["ips"]) else { return }
                    arguments += ["--ips", ips.path]
                }
            }
            let process = Process()
            process.executableURL = Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/ssd-recompiled")
            process.arguments = arguments
            process.currentDirectoryURL = base
            var environment = ProcessInfo.processInfo.environment
            environment["SSD_DATA_DIR"] = base.path
            process.environment = environment
            let log = base.appendingPathComponent("Last Run.log")
            try Data().write(to: log)
            let handle = try FileHandle(forWritingTo: log)
            defer { try? handle.close() }
            process.standardError = handle
            process.standardOutput = handle
            try process.run()

            // SDL creates its NSApplication asynchronously. Retry briefly, then stop stealing focus.
            let focusDeadline = Date().addingTimeInterval(10)
            var focused = false
            while process.isRunning {
                if !focused, Date() < focusDeadline,
                   let application = NSRunningApplication(processIdentifier: process.processIdentifier) {
                    if application.isActive { focused = true }
                    else { _ = application.activate(options: [.activateIgnoringOtherApps]) }
                }
                RunLoop.current.run(until: Date().addingTimeInterval(0.1))
            }
            process.waitUntilExit()
            if process.terminationStatus != 0 {
                _ = dialog("遊戲已停止", "請查看記錄檔以了解詳細資訊：\n" + log.path, ["確定"])
            }
            exit(process.terminationStatus)
        } catch {
            _ = dialog("無法啟動遊戲", error.localizedDescription, ["確定"])
            exit(1)
        }
    }
}
