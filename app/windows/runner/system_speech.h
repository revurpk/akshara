#ifndef RUNNER_SYSTEM_SPEECH_H_
#define RUNNER_SYSTEM_SPEECH_H_

#include <flutter/binary_messenger.h>
#include <flutter/encodable_value.h>
#include <flutter/method_channel.h>
#include <windows.h>
#include <sapi.h>
#include <wrl/client.h>

#include <memory>
#include <string>

// The system speech recogniser for voice input (lib/assist/system_speech.dart
// describes the 'akshara/speech' channel). It uses the SAPI desktop
// recogniser (Windows Speech Recognition's engine), which runs entirely on
// this computer; the online WinRT dictation service is deliberately not used.
class SystemSpeech {
 public:
  explicit SystemSpeech(flutter::BinaryMessenger* messenger);
  ~SystemSpeech();

  SystemSpeech(const SystemSpeech&) = delete;
  SystemSpeech& operator=(const SystemSpeech&) = delete;

 private:
  using Result = std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>>;

  void Handle(const flutter::MethodCall<flutter::EncodableValue>& call, Result result);
  flutter::EncodableValue Available();
  std::string Start(const std::string& language);
  void Stop();
  void Cancel();
  flutter::EncodableValue Status();
  void DrainEvents();

  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>> channel_;
  Microsoft::WRL::ComPtr<ISpRecognizer> recognizer_;
  Microsoft::WRL::ComPtr<ISpRecoContext> context_;
  Microsoft::WRL::ComPtr<ISpRecoGrammar> grammar_;

  enum class State { kIdle, kRecording, kTranscribing };
  State state_ = State::kIdle;
  std::string text_;
  ULONGLONG started_ = 0;
  ULONGLONG stopped_ = 0;
  bool stream_ended_ = false;
};

#endif  // RUNNER_SYSTEM_SPEECH_H_
