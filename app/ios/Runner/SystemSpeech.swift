// The system speech recogniser for voice input (lib/assist/system_speech.dart
// describes the 'akshara/speech' channel). Apple's SFSpeechRecognizer runs
// with requiresOnDeviceRecognition, so audio never leaves the device; a
// language without an on-device model is reported as unavailable rather
// than sent to Apple's servers.
//
// The same file is used by the macOS and iOS runners (keep both copies
// identical: macos/Runner and ios/Runner).

import AVFoundation
import Speech

#if os(macOS)
import FlutterMacOS
#else
import Flutter
#endif

final class SystemSpeech: NSObject {
  private static let locales: [(String, String)] = [
    ("en", "en-US"), ("te", "te-IN"), ("hi", "hi-IN"), ("sa", "sa-IN"),
  ]

  private let audio = AVAudioEngine()
  private var request: SFSpeechAudioBufferRecognitionRequest?
  private var task: SFSpeechRecognitionTask?
  private var channel: FlutterMethodChannel?

  // Session state, read by `status`. Only touched on the main thread except
  // `level`, which the audio tap writes under `lock`.
  private var generation = 0
  private var state = "idle"
  private var text = ""
  private var error: String?
  private var started = Date()
  private let lock = NSLock()
  private var level: Float = 0

  func register(messenger: FlutterBinaryMessenger) {
    let channel = FlutterMethodChannel(name: "akshara/speech", binaryMessenger: messenger)
    channel.setMethodCallHandler { [weak self] call, result in
      guard let self = self else { return result(FlutterMethodNotImplemented) }
      self.handle(call, result)
    }
    self.channel = channel
  }

  private func handle(_ call: FlutterMethodCall, _ result: @escaping FlutterResult) {
    switch call.method {
    case "available":
      result(available())
    case "start":
      let args = call.arguments as? [String: Any]
      start(language: args?["language"] as? String ?? "auto", result)
    case "stop":
      stop()
      result(nil)
    case "cancel":
      cancel()
      result(nil)
    case "status":
      result(status())
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  private func available() -> [String: Any] {
    let auth = SFSpeechRecognizer.authorizationStatus()
    if auth == .denied || auth == .restricted {
      return ["available": false, "languages": [String](),
              "reason": "Speech recognition is turned off for Akshara in the system settings (Privacy & Security)."]
    }
    var languages: [String] = []
    for (code, id) in Self.locales {
      if let r = SFSpeechRecognizer(locale: Locale(identifier: id)), r.supportsOnDeviceRecognition {
        languages.append(code)
      }
    }
    let systemOnDevice = SFSpeechRecognizer()?.supportsOnDeviceRecognition ?? false
    return ["available": !languages.isEmpty || systemOnDevice, "languages": languages]
  }

  private func recognizer(for language: String) -> SFSpeechRecognizer? {
    if let id = Self.locales.first(where: { $0.0 == language })?.1 {
      return SFSpeechRecognizer(locale: Locale(identifier: id))
    }
    return SFSpeechRecognizer()
  }

  private func start(language: String, _ result: @escaping FlutterResult) {
    cancel()
    generation += 1
    let gen = generation
    SFSpeechRecognizer.requestAuthorization { auth in
      DispatchQueue.main.async {
        guard auth == .authorized else {
          return result(FlutterError(code: "denied", message:
            "Akshara may not use speech recognition. Allow it in the system settings (Privacy & Security → Speech Recognition).", details: nil))
        }
        AVCaptureDevice.requestAccess(for: .audio) { granted in
          DispatchQueue.main.async {
            guard granted else {
              return result(FlutterError(code: "denied", message:
                "Akshara may not use the microphone. Allow it in the system settings (Privacy & Security → Microphone).", details: nil))
            }
            guard gen == self.generation else { return result(nil) }  // cancelled meanwhile
            do {
              try self.begin(language: language)
              result(nil)
            } catch {
              self.teardown()
              result(FlutterError(code: "failed", message: error.localizedDescription, details: nil))
            }
          }
        }
      }
    }
  }

  private func begin(language: String) throws {
    guard let recognizer = recognizer(for: language), recognizer.isAvailable else {
      throw NSError(domain: "akshara", code: 1, userInfo: [NSLocalizedDescriptionKey: "The system speech recogniser is not available right now."])
    }
    guard recognizer.supportsOnDeviceRecognition else {
      throw NSError(domain: "akshara", code: 2, userInfo: [NSLocalizedDescriptionKey:
        "The system cannot recognise this language offline. Download it in the system's dictation settings, or use the built-in Whisper engine."])
    }
    #if os(iOS)
    let session = AVAudioSession.sharedInstance()
    try session.setCategory(.record, mode: .measurement, options: .duckOthers)
    try session.setActive(true, options: .notifyOthersOnDeactivation)
    #endif

    let request = SFSpeechAudioBufferRecognitionRequest()
    request.requiresOnDeviceRecognition = true  // never send audio to a server
    request.shouldReportPartialResults = true
    if #available(macOS 13, iOS 16, *) { request.addsPunctuation = true }
    self.request = request

    let input = audio.inputNode
    let format = input.outputFormat(forBus: 0)
    input.removeTap(onBus: 0)
    input.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
      request.append(buffer)
      self?.measure(buffer)
    }
    audio.prepare()
    try audio.start()

    text = ""
    error = nil
    started = Date()
    state = "recording"
    let gen = generation
    task = recognizer.recognitionTask(with: request) { [weak self] res, err in
      DispatchQueue.main.async { self?.update(gen, res, err) }
    }
  }

  private func measure(_ buffer: AVAudioPCMBuffer) {
    guard let samples = buffer.floatChannelData?[0] else { return }
    let n = Int(buffer.frameLength)
    guard n > 0 else { return }
    var sum: Float = 0
    for i in 0..<n { sum += samples[i] * samples[i] }
    let rms = (sum / Float(n)).squareRoot()
    lock.lock()
    level = rms
    lock.unlock()
  }

  private func update(_ gen: Int, _ res: SFSpeechRecognitionResult?, _ err: Error?) {
    guard gen == generation, state == "recording" || state == "transcribing" else { return }
    if let res = res {
      text = res.bestTranscription.formattedString
      if res.isFinal {
        state = "done"
        teardown()
        return
      }
    }
    if let err = err {
      // "No speech detected" and similar end a session that has nothing to say.
      if state == "transcribing" || !text.isEmpty || (err as NSError).code == 1110 {
        state = "done"
      } else {
        state = "error"
        error = err.localizedDescription
      }
      teardown()
    }
  }

  private func stop() {
    guard state == "recording" else { return }
    state = "transcribing"
    audio.stop()
    audio.inputNode.removeTap(onBus: 0)
    request?.endAudio()  // the final result arrives in `update`
  }

  private func cancel() {
    generation += 1
    task?.cancel()
    teardown()
    state = "idle"
  }

  private func teardown() {
    if audio.isRunning { audio.stop() }
    audio.inputNode.removeTap(onBus: 0)
    request = nil
    task = nil
    #if os(iOS)
    try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    #endif
  }

  private func status() -> [String: Any] {
    switch state {
    case "recording":
      lock.lock()
      let l = level
      lock.unlock()
      return ["state": "recording", "level": Double(l), "seconds": Date().timeIntervalSince(started)]
    case "transcribing":
      return ["state": "transcribing"]
    case "error":
      state = "idle"
      return ["state": "error", "error": error ?? "speech recognition failed"]
    default:  // done, reported once; idle reports an empty result
      let result = state == "done" ? text : ""
      state = "idle"
      text = ""
      return ["state": "done", "text": result]
    }
  }
}
