import AppKit
import QuartzCore
import Darwin
import ScreenSaver

private struct MatrixSettings {
    struct Version {
        let id: String
        let title: String
    }

    static let versions = [
        Version(id: "classic", title: "Classic"),
        Version(id: "3d", title: "3D"),
        Version(id: "neomatrixology", title: "Neomatrixology"),
        Version(id: "megacity", title: "Megacity"),
        Version(id: "operator", title: "Operator"),
        Version(id: "resurrections", title: "Resurrections"),
        Version(id: "paradise", title: "Paradise"),
        Version(id: "nightmare", title: "Nightmare"),
        Version(id: "trinity", title: "Trinity"),
        Version(id: "morpheus", title: "Morpheus"),
        Version(id: "bugs", title: "Bugs"),
    ]

    var versionID = "classic"
    var mirrorEnabled = false
    var skipIntro = false

    private static var store: ScreenSaverDefaults? {
        let module = Bundle(for: MatrixView.self).bundleIdentifier ?? "io.github.arctan95.matrix"
        return ScreenSaverDefaults(forModuleWithName: module)
    }

    static func index(for versionID: String) -> Int {
        versions.firstIndex { $0.id == versionID } ?? 0
    }

    static func versionID(at index: Int) -> String {
        versions[min(max(index, 0), versions.count - 1)].id
    }

    static func load() -> MatrixSettings {
        var settings = MatrixSettings()
        guard let store else { return settings }

        store.register(defaults: [
            "versionID": settings.versionID,
            "mirrorEnabled": settings.mirrorEnabled,
            "skipIntro": settings.skipIntro,
        ])

        if let versionID = store.string(forKey: "versionID"), versions.contains(where: { $0.id == versionID }) {
            settings.versionID = versionID
        }
        settings.mirrorEnabled = store.bool(forKey: "mirrorEnabled")
        settings.skipIntro = store.bool(forKey: "skipIntro")
        return settings
    }

    func save() {
        guard let store = Self.store else { return }

        store.set(versionID, forKey: "versionID")
        store.set(mirrorEnabled, forKey: "mirrorEnabled")
        store.set(skipIntro, forKey: "skipIntro")
        store.synchronize()
    }
}

@objc(matrixView)
final class MatrixView: ScreenSaverView {
    private static let screenSaverWillStopNotification = Notification.Name("com.apple.screensaver.willstop")

    private let previewMode: Bool
    private var renderer: UnsafeMutableRawPointer?
    private var lastBackingSize = CGSize.zero
    private var sheet: NSWindow?
    private var versionPopup: NSPopUpButton?
    private var mirrorCheckbox: NSButton?
    private var skipIntroCheckbox: NSButton?

    override init?(frame: NSRect, isPreview: Bool) {
        previewMode = isPreview
        super.init(frame: frame, isPreview: isPreview)
        setup()
    }

    required init?(coder: NSCoder) {
        previewMode = false
        super.init(coder: coder)
        setup()
    }

    deinit {
        DistributedNotificationCenter.default().removeObserver(self)
        stopRenderer()
    }

    private func setup() {
        animationTimeInterval = 1.0 / 60.0
        wantsLayer = true
        layer?.backgroundColor = NSColor.black.cgColor

        if !previewMode {
            DistributedNotificationCenter.default().addObserver(
                self,
                selector: #selector(screenSaverWillStop(_:)),
                name: Self.screenSaverWillStopNotification,
                object: nil
            )
        }
    }

    override func startAnimation() {
        super.startAnimation()

        startRenderer()
        fadeIn()
    }

    override func animateOneFrame() {
        renderFrame()
    }

    override func stopAnimation() {
        stopRenderer()
        super.stopAnimation()
    }

    override var hasConfigureSheet: Bool {
        true
    }

    override var configureSheet: NSWindow? {
        if sheet == nil {
            sheet = buildSheet()
        }

        let settings = MatrixSettings.load()
        versionPopup?.selectItem(at: MatrixSettings.index(for: settings.versionID))
        mirrorCheckbox?.state = settings.mirrorEnabled ? .on : .off
        skipIntroCheckbox?.state = settings.skipIntro ? .on : .off
        return sheet
    }

    private func startRenderer() {
        let settings = MatrixSettings.load()
        guard renderer == nil else {
            applySettings()
            resizeRendererIfNeeded()
            return
        }

        let size = currentBackingSize()
        lastBackingSize = size
        settings.versionID.withCString { version in
            renderer = matrix_saver_new(
                Unmanaged.passUnretained(self).toOpaque(),
                UInt32(size.width),
                UInt32(size.height),
                version,
                settings.mirrorEnabled ? 1 : 0,
                settings.skipIntro ? 1 : 0
            )
        }
    }

    private func stopRenderer() {
        if let renderer {
            matrix_saver_free(renderer)
            self.renderer = nil
        }
    }

    private func applySettings() {
        guard let renderer else { return }

        let settings = MatrixSettings.load()
        settings.versionID.withCString { version in
            matrix_saver_apply_settings(
                renderer,
                version,
                settings.mirrorEnabled ? 1 : 0,
                settings.skipIntro ? 1 : 0
            )
        }
    }

    private func renderFrame() {
        guard let renderer else { return }

        resizeRendererIfNeeded()
        matrix_saver_render(renderer)
    }

    private func resizeRendererIfNeeded() {
        guard let renderer else { return }

        let size = currentBackingSize()
        if size != lastBackingSize {
            lastBackingSize = size
            matrix_saver_resize(renderer, UInt32(size.width), UInt32(size.height))
        }
    }

    private func currentBackingSize() -> CGSize {
        let backingBounds = convertToBacking(bounds)
        return CGSize(width: max(1, floor(backingBounds.width)), height: max(1, floor(backingBounds.height)))
    }

    private func fadeIn() {
        guard let layer else { return }

        layer.opacity = 0.0
        let animation = CABasicAnimation(keyPath: "opacity")
        animation.fromValue = 0.0
        animation.toValue = 1.0
        animation.duration = 2.0
        animation.fillMode = .forwards
        animation.isRemovedOnCompletion = false
        layer.add(animation, forKey: "fadeAnimation")
    }

    private func buildSheet() -> NSWindow {
        let version = NSPopUpButton(frame: .zero, pullsDown: false)
        version.addItems(withTitles: MatrixSettings.versions.map { $0.title })
        versionPopup = version

        let mirror = NSButton(checkboxWithTitle: "Enable mirror effect", target: nil, action: nil)
        mirrorCheckbox = mirror

        let skipIntro = NSButton(checkboxWithTitle: "Skip intro", target: nil, action: nil)
        skipIntroCheckbox = skipIntro

        func label(_ string: String) -> NSTextField {
            NSTextField(labelWithString: string)
        }

        let grid = NSGridView(views: [
            [label("Version:"), version],
            [label("Effect:"), mirror],
            [label("Intro:"), skipIntro],
        ])
        grid.rowSpacing = 14
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        for row in 0..<grid.numberOfRows {
            grid.row(at: row).yPlacement = .center
        }

        let cancel = NSButton(title: "Cancel", target: self, action: #selector(sheetCancel(_:)))
        cancel.keyEquivalent = "\u{1b}"
        let ok = NSButton(title: "OK", target: self, action: #selector(sheetOK(_:)))
        ok.keyEquivalent = "\r"

        let buttons = NSStackView(views: [cancel, ok])
        buttons.orientation = .horizontal
        buttons.spacing = 12

        let content = NSView()
        for view in [grid, buttons] {
            view.translatesAutoresizingMaskIntoConstraints = false
            content.addSubview(view)
        }

        NSLayoutConstraint.activate([
            version.widthAnchor.constraint(equalToConstant: 220),
            mirror.widthAnchor.constraint(equalTo: version.widthAnchor),
            skipIntro.widthAnchor.constraint(equalTo: version.widthAnchor),
            ok.widthAnchor.constraint(greaterThanOrEqualToConstant: 76),
            cancel.widthAnchor.constraint(greaterThanOrEqualToConstant: 76),
            grid.topAnchor.constraint(equalTo: content.topAnchor, constant: 24),
            grid.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 24),
            grid.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -24),
            buttons.topAnchor.constraint(equalTo: grid.bottomAnchor, constant: 24),
            buttons.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -24),
            buttons.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -20),
        ])

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 380, height: 190),
            styleMask: [.titled],
            backing: .buffered,
            defer: true
        )
        window.title = "Matrix"
        window.contentView = content
        window.setContentSize(content.fittingSize)
        return window
    }

    @objc private func sheetOK(_ sender: Any?) {
        var settings = MatrixSettings.load()
        settings.versionID = MatrixSettings.versionID(
            at: versionPopup?.indexOfSelectedItem ?? MatrixSettings.index(for: settings.versionID)
        )
        settings.mirrorEnabled = mirrorCheckbox?.state == .on
        settings.skipIntro = skipIntroCheckbox?.state == .on
        settings.save()

        applySettings()
        dismissSheet(.OK)
    }

    @objc private func sheetCancel(_ sender: Any?) {
        dismissSheet(.cancel)
    }

    private func dismissSheet(_ response: NSApplication.ModalResponse) {
        guard let sheet else { return }

        if let parent = sheet.sheetParent {
            parent.endSheet(sheet, returnCode: response)
        } else {
            sheet.close()
        }
    }

    @objc private func screenSaverWillStop(_ notification: Notification) {
        stopRenderer()
        Darwin.exit(0)
    }
}
