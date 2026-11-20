#include "system_speech.h"

#include <flutter/standard_method_codec.h>

#include <cstdio>
#include <vector>

using Microsoft::WRL::ComPtr;
using flutter::EncodableList;
using flutter::EncodableMap;
using flutter::EncodableValue;

namespace {

// Voice languages offered by the app, as SAPI primary language ids.
struct Language {
  const char* code;
  WORD primary;
};
constexpr Language kLanguages[] = {
    {"en", LANG_ENGLISH}, {"te", LANG_TELUGU}, {"hi", LANG_HINDI}, {"sa", LANG_SANSKRIT}};

// How long to wait for the recogniser to finish after the microphone stops.
constexpr ULONGLONG kFinishTimeoutMs = 4000;

std::string Utf8(const wchar_t* s) {
  if (!s || !*s) return {};
  int n = WideCharToMultiByte(CP_UTF8, 0, s, -1, nullptr, 0, nullptr, nullptr);
  if (n <= 1) return {};
  std::string out(static_cast<size_t>(n - 1), '\0');
  WideCharToMultiByte(CP_UTF8, 0, s, -1, out.data(), n, nullptr, nullptr);
  return out;
}

struct Recognizer {
  ComPtr<ISpObjectToken> token;
  std::vector<WORD> primaries;  // primary language ids it understands
};

ComPtr<ISpObjectTokenCategory> Category(const wchar_t* id) {
  ComPtr<ISpObjectTokenCategory> cat;
  if (FAILED(CoCreateInstance(CLSID_SpObjectTokenCategory, nullptr, CLSCTX_ALL, IID_PPV_ARGS(&cat))) ||
      FAILED(cat->SetId(id, FALSE))) {
    return nullptr;
  }
  return cat;
}

// The installed SAPI recognisers and their languages (the token's
// Attributes\Language value, hex LANGIDs separated by ';').
std::vector<Recognizer> Recognizers() {
  std::vector<Recognizer> out;
  auto cat = Category(SPCAT_RECOGNIZERS);
  ComPtr<IEnumSpObjectTokens> tokens;
  if (!cat || FAILED(cat->EnumTokens(nullptr, nullptr, &tokens))) return out;
  ULONG count = 0;
  tokens->GetCount(&count);
  for (ULONG i = 0; i < count; i++) {
    Recognizer r;
    if (FAILED(tokens->Item(i, &r.token))) continue;
    ComPtr<ISpDataKey> attrs;
    wchar_t* langs = nullptr;
    if (SUCCEEDED(r.token->OpenKey(L"Attributes", &attrs)) &&
        SUCCEEDED(attrs->GetStringValue(L"Language", &langs)) && langs) {
      std::wstring list(langs);
      CoTaskMemFree(langs);
      size_t start = 0;
      while (start <= list.size()) {
        size_t end = list.find(L';', start);
        if (end == std::wstring::npos) end = list.size();
        std::wstring part = list.substr(start, end - start);
        if (!part.empty()) {
          WORD id = static_cast<WORD>(wcstoul(part.c_str(), nullptr, 16));
          r.primaries.push_back(PRIMARYLANGID(id));
        }
        start = end + 1;
      }
    }
    out.push_back(std::move(r));
  }
  return out;
}

bool Understands(const Recognizer& r, WORD primary) {
  for (WORD p : r.primaries) {
    if (p == primary) return true;
  }
  return false;
}

void ClearEvent(SPEVENT& ev) {
  switch (ev.elParamType) {
    case SPET_LPARAM_IS_OBJECT:
    case SPET_LPARAM_IS_TOKEN:
      if (ev.lParam) reinterpret_cast<IUnknown*>(ev.lParam)->Release();
      break;
    case SPET_LPARAM_IS_POINTER:
    case SPET_LPARAM_IS_STRING:
      if (ev.lParam) CoTaskMemFree(reinterpret_cast<void*>(ev.lParam));
      break;
    default:
      break;
  }
  ev.lParam = 0;
}

}  // namespace

SystemSpeech::SystemSpeech(flutter::BinaryMessenger* messenger)
    : channel_(std::make_unique<flutter::MethodChannel<EncodableValue>>(
          messenger, "akshara/speech", &flutter::StandardMethodCodec::GetInstance())) {
  channel_->SetMethodCallHandler(
      [this](const flutter::MethodCall<EncodableValue>& call, Result result) {
        Handle(call, std::move(result));
      });
}

SystemSpeech::~SystemSpeech() {
  channel_->SetMethodCallHandler(nullptr);
  Cancel();
}

void SystemSpeech::Handle(const flutter::MethodCall<EncodableValue>& call, Result result) {
  const std::string& method = call.method_name();
  if (method == "available") {
    result->Success(Available());
  } else if (method == "start") {
    std::string language = "auto";
    if (const auto* args = std::get_if<EncodableMap>(call.arguments())) {
      auto it = args->find(EncodableValue("language"));
      if (it != args->end()) {
        if (const auto* s = std::get_if<std::string>(&it->second)) language = *s;
      }
    }
    std::string error = Start(language);
    if (error.empty()) {
      result->Success();
    } else {
      result->Error("failed", error);
    }
  } else if (method == "stop") {
    Stop();
    result->Success();
  } else if (method == "cancel") {
    Cancel();
    result->Success();
  } else if (method == "status") {
    result->Success(Status());
  } else {
    result->NotImplemented();
  }
}

EncodableValue SystemSpeech::Available() {
  auto recognizers = Recognizers();
  EncodableList languages;
  for (const auto& lang : kLanguages) {
    for (const auto& r : recognizers) {
      if (Understands(r, lang.primary)) {
        languages.emplace_back(std::string(lang.code));
        break;
      }
    }
  }
  return EncodableValue(EncodableMap{
      {EncodableValue("available"), EncodableValue(!recognizers.empty())},
      {EncodableValue("languages"), EncodableValue(languages)},
  });
}

std::string SystemSpeech::Start(const std::string& language) {
  Cancel();
  auto recognizers = Recognizers();
  if (recognizers.empty()) return "Windows has no speech recogniser installed.";

  // The recogniser for the chosen language; 'auto' prefers the Windows
  // display language, then the first one installed.
  const Recognizer* chosen = nullptr;
  WORD want = PRIMARYLANGID(GetUserDefaultUILanguage());
  for (const auto& lang : kLanguages) {
    if (language == lang.code) want = lang.primary;
  }
  for (const auto& r : recognizers) {
    if (Understands(r, want)) {
      chosen = &r;
      break;
    }
  }
  if (!chosen) {
    if (language != "auto") return "Windows has no offline speech recogniser for this language.";
    chosen = &recognizers.front();
  }

  HRESULT hr = CoCreateInstance(CLSID_SpInprocRecognizer, nullptr, CLSCTX_ALL, IID_PPV_ARGS(&recognizer_));
  if (SUCCEEDED(hr)) hr = recognizer_->SetRecognizer(chosen->token.Get());

  // The default microphone.
  ComPtr<ISpObjectToken> input;
  if (SUCCEEDED(hr)) {
    auto cat = Category(SPCAT_AUDIOIN);
    wchar_t* id = nullptr;
    hr = cat ? cat->GetDefaultTokenId(&id) : E_FAIL;
    if (SUCCEEDED(hr)) hr = CoCreateInstance(CLSID_SpObjectToken, nullptr, CLSCTX_ALL, IID_PPV_ARGS(&input));
    if (SUCCEEDED(hr)) hr = input->SetId(nullptr, id, FALSE);
    if (id) CoTaskMemFree(id);
    if (FAILED(hr)) {
      Cancel();
      return "No microphone was found.";
    }
    hr = recognizer_->SetInput(input.Get(), TRUE);
  }

  if (SUCCEEDED(hr)) hr = recognizer_->CreateRecoContext(&context_);
  if (SUCCEEDED(hr)) {
    const ULONGLONG interest = SPFEI(SPEI_RECOGNITION) | SPFEI(SPEI_END_SR_STREAM);
    hr = context_->SetInterest(interest, interest);
  }
  if (SUCCEEDED(hr)) hr = context_->CreateGrammar(1, &grammar_);
  if (SUCCEEDED(hr)) hr = grammar_->LoadDictation(nullptr, SPLO_STATIC);
  if (SUCCEEDED(hr)) hr = grammar_->SetDictationState(SPRS_ACTIVE);
  if (SUCCEEDED(hr)) hr = recognizer_->SetRecoState(SPRST_ACTIVE);
  if (FAILED(hr)) {
    Cancel();
    char buf[96];
    snprintf(buf, sizeof buf, "The Windows speech recogniser could not be started (error 0x%08lX).",
             static_cast<unsigned long>(hr));
    return buf;
  }
  text_.clear();
  stream_ended_ = false;
  started_ = GetTickCount64();
  state_ = State::kRecording;
  return {};
}

void SystemSpeech::Stop() {
  if (state_ != State::kRecording) return;
  DrainEvents();
  // Closes the microphone; audio already captured is still recognised and
  // ends with SPEI_END_SR_STREAM.
  if (recognizer_) recognizer_->SetRecoState(SPRST_INACTIVE);
  stopped_ = GetTickCount64();
  state_ = State::kTranscribing;
}

void SystemSpeech::Cancel() {
  if (grammar_) grammar_->SetDictationState(SPRS_INACTIVE);
  if (recognizer_) recognizer_->SetRecoState(SPRST_INACTIVE_WITH_PURGE);
  if (context_) {
    SPEVENT ev;
    ULONG fetched = 0;
    while (context_->GetEvents(1, &ev, &fetched) == S_OK && fetched == 1) ClearEvent(ev);
  }
  grammar_.Reset();
  context_.Reset();
  recognizer_.Reset();
  state_ = State::kIdle;
}

void SystemSpeech::DrainEvents() {
  if (!context_) return;
  SPEVENT ev;
  ULONG fetched = 0;
  while (SUCCEEDED(context_->GetEvents(1, &ev, &fetched)) && fetched == 1) {
    if (ev.eEventId == SPEI_RECOGNITION && ev.elParamType == SPET_LPARAM_IS_OBJECT && ev.lParam) {
      auto* res = reinterpret_cast<ISpRecoResult*>(ev.lParam);
      wchar_t* phrase = nullptr;
      if (SUCCEEDED(res->GetText(SP_GETWHOLEPHRASE, SP_GETWHOLEPHRASE, TRUE, &phrase, nullptr)) && phrase) {
        std::string s = Utf8(phrase);
        CoTaskMemFree(phrase);
        if (!s.empty()) {
          if (!text_.empty()) text_ += ' ';
          text_ += s;
        }
      }
    } else if (ev.eEventId == SPEI_END_SR_STREAM) {
      stream_ended_ = true;
    }
    ClearEvent(ev);
    fetched = 0;
  }
}

EncodableValue SystemSpeech::Status() {
  DrainEvents();
  switch (state_) {
    case State::kRecording: {
      double level = 0;
      SPRECOGNIZERSTATUS st = {};
      if (recognizer_ && SUCCEEDED(recognizer_->GetStatus(&st))) {
        level = st.AudioStatus.dwAudioLevel / 300.0;  // 0–100 → the 0–0.33 RMS range
      }
      return EncodableValue(EncodableMap{
          {EncodableValue("state"), EncodableValue("recording")},
          {EncodableValue("level"), EncodableValue(level)},
          {EncodableValue("seconds"), EncodableValue((GetTickCount64() - started_) / 1000.0)},
      });
    }
    case State::kTranscribing:
      if (!stream_ended_ && GetTickCount64() - stopped_ < kFinishTimeoutMs) {
        return EncodableValue(EncodableMap{{EncodableValue("state"), EncodableValue("transcribing")}});
      }
      [[fallthrough]];
    default: {
      std::string text = state_ == State::kTranscribing ? text_ : std::string();
      Cancel();
      text_.clear();
      return EncodableValue(EncodableMap{
          {EncodableValue("state"), EncodableValue("done")},
          {EncodableValue("text"), EncodableValue(text)},
      });
    }
  }
}
