import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  private let speech = SystemSpeech()

  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)
    speech.register(messenger: flutterViewController.engine.binaryMessenger)

    super.awakeFromNib()
  }
}
